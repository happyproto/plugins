//! Library calls in flight, and the loop that settles them.
//!
//! Every call a script makes is started at once and handed back as a
//! promise; the host runs it beside the guest. The guest is single-threaded,
//! so a promise can only settle while this loop holds the thread: it runs
//! QuickJS's job queue dry, and when the promise it is waiting for is still
//! pending it blocks in the host until *any* call settles, resolves that
//! call's promise, and runs the queue again. `Promise.all` over three calls
//! therefore costs the slowest of them rather than their sum.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use happyview_plugin_sdk::{PluginError, Value as Json};
use rquickjs::promise::PromiseState;
use rquickjs::{Ctx, Exception, Function, Persistent, Promise, Value};

use crate::backend::Backend;
use crate::budget::Budget;
use crate::convert;

/// The code the host uses when the message is already its own.
///
/// A discriminator rather than a guarantee: nothing stops a library from
/// returning this code as its own error, and the envelope carries no field
/// saying which side wrote the message, so such a library gets the unprefixed
/// rendering where the native path would prefix it. The Lua bridge reads it
/// the same way, which is the point.
const HOST_RENDERED: &str = "LIBRARY_ERROR";

/// A library's failure as the `Error` its promise rejects with. The message
/// is the Lua bridge's rendering of the same envelope, word for word, so a
/// script matching on text inside it — and the host looking for an
/// `AUTH_ERROR:` prefix to answer 401 with — reads the same thing whichever
/// language raised it. `code` and `retryable` are there to be read without
/// parsing.
///
/// Two shapes, because the host sends two. A library that returned an error
/// of its own arrives as a code and a message, which the host displays as
/// `Plugin returned error: {code} - {message}`. Everything else — a depth
/// limit, a library that is not installed, one that trapped — arrives under
/// `LIBRARY_ERROR` carrying the host's own rendering, shown unprefixed.
pub fn library_error<'js>(
    ctx: &Ctx<'js>,
    label: &str,
    error: &PluginError,
) -> rquickjs::Result<Value<'js>> {
    let message = if error.code == HOST_RENDERED {
        format!("{label}: {}", error.message)
    } else {
        format!(
            "{label}: Plugin returned error: {} - {}",
            error.code, error.message
        )
    };
    let exception = Exception::from_message(ctx.clone(), &message)?;
    exception.set("code", error.code.as_str())?;
    exception.set("retryable", error.retryable)?;
    Ok(exception.into_value())
}

/// One call's promise, kept outside the JavaScript heap until it settles.
struct Pending {
    label: String,
    resolve: Persistent<Function<'static>>,
    reject: Persistent<Function<'static>>,
}

pub struct Calls {
    backend: Rc<dyn Backend>,
    pending: RefCell<BTreeMap<u32, Pending>>,
}

/// How waiting on a promise ended, when it did not end in a value.
pub enum Unsettled {
    /// The promise rejected, or a job threw. The thrown value is pending on
    /// the context, as `rquickjs::Error::Exception` always means.
    Thrown(rquickjs::Error),
    /// Nothing that could settle the promise is left: the queue is dry and
    /// no call is in flight.
    Stuck,
    /// The budget ran out while the loop ran.
    Spent,
    /// The host refused the wait itself — a handle it never issued, which is
    /// this plugin failing rather than the script.
    Host(PluginError),
}

impl Calls {
    pub fn new(backend: Rc<dyn Backend>) -> Self {
        Self {
            backend,
            pending: RefCell::new(BTreeMap::new()),
        }
    }

    /// Start a call and hand back its promise. A failure to start — `args`
    /// that would not convert, or a host that could not read the request —
    /// rejects the promise rather than throwing, so a sending call fails the
    /// one way an `async` function does.
    pub fn send<'js>(
        &self,
        ctx: &Ctx<'js>,
        library: &str,
        function: &str,
        label: &str,
        args: rquickjs::Result<Vec<Json>>,
    ) -> rquickjs::Result<Promise<'js>> {
        let (promise, resolve, reject) = ctx.promise()?;
        let args = match args {
            Ok(args) => args,
            Err(rquickjs::Error::Exception) => {
                reject.call::<_, ()>((ctx.catch(),))?;
                return Ok(promise);
            }
            Err(other) => return Err(other),
        };
        match self.backend.start(library, function, &args) {
            Ok(handle) => {
                self.pending.borrow_mut().insert(
                    handle,
                    Pending {
                        label: label.to_string(),
                        resolve: Persistent::save(ctx, resolve),
                        reject: Persistent::save(ctx, reject),
                    },
                );
            }
            Err(error) => reject.call::<_, ()>((library_error(ctx, label, &error)?,))?,
        }
        Ok(promise)
    }

    /// Drive the queue and the host until `promise` settles.
    pub fn run<'js>(
        &self,
        ctx: &Ctx<'js>,
        promise: &Promise<'js>,
        budget: &Budget,
    ) -> Result<Value<'js>, Unsettled> {
        loop {
            // A job that throws is one the budget interrupted — nothing else
            // escapes a promise job — so a spent budget is checked before the
            // next job rather than after the queue is dry.
            while !budget.is_spent() && ctx.execute_pending_job() {}
            if budget.is_spent() {
                return Err(Unsettled::Spent);
            }
            match promise.state() {
                PromiseState::Resolved | PromiseState::Rejected => {
                    return match promise.result::<Value>() {
                        Some(Ok(value)) => Ok(value),
                        Some(Err(error)) => Err(Unsettled::Thrown(error)),
                        None => unreachable!("the state was read as settled"),
                    };
                }
                PromiseState::Pending => {}
            }
            self.settle_one(ctx)?;
        }
    }

    /// Block in the host until one call settles, and settle its promise.
    fn settle_one(&self, ctx: &Ctx<'_>) -> Result<(), Unsettled> {
        let handles: Vec<u32> = self.pending.borrow().keys().copied().collect();
        if handles.is_empty() {
            return Err(Unsettled::Stuck);
        }
        let (handle, outcome) = self.backend.wait_any(&handles).map_err(Unsettled::Host)?;
        let Some(call) = self.pending.borrow_mut().remove(&handle) else {
            return Err(Unsettled::Host(PluginError::host(format!(
                "the host settled handle {handle}, which is not in flight"
            ))));
        };
        let settled = (|| -> rquickjs::Result<()> {
            match outcome {
                Ok(value) => match convert::to_js(ctx, &value) {
                    Ok(value) => call.resolve.restore(ctx)?.call((value,)),
                    // A result too large for the memory ceiling fails the
                    // call that asked for it, where the script can see it.
                    Err(rquickjs::Error::Exception) => {
                        call.reject.restore(ctx)?.call((ctx.catch(),))
                    }
                    Err(other) => Err(other),
                },
                Err(error) => {
                    call.reject
                        .restore(ctx)?
                        .call((library_error(ctx, &call.label, &error)?,))
                }
            }
        })();
        settled.map_err(Unsettled::Thrown)
    }

    /// Forget every call still in flight. Each holds its promise's two
    /// functions outside the JavaScript heap, where QuickJS cannot see them
    /// to collect, and a runtime freed with them still held aborts. The host
    /// drains or aborts the calls themselves; this lets go of their promises.
    pub fn clear(&self) {
        self.pending.borrow_mut().clear();
    }
}

/// Clears `calls` when dropped, so every way out of a run — a `?` included —
/// lets go of the pending promises while the runtime is still alive.
pub struct ClearOnDrop<'a>(pub &'a Calls);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.clear();
    }
}

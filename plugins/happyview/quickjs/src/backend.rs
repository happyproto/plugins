//! Everything this plugin asks of the host, behind one trait. The interpreter
//! never names an SDK host wrapper itself: a native test cannot reach the
//! host, and the event loop is only worth testing against a host that settles
//! calls in an order the test chose.

use happyview_plugin_sdk::{
    host, ApiSurface, JobProgressRequest, PluginError, ScriptLogRequest, Value as Json,
};

pub trait Backend {
    /// A library's exports, read once per import.
    fn library_surface(&self, library: &str) -> Result<ApiSurface, PluginError>;

    /// Start a call and return its handle at once. `Err` only when the host
    /// could not read the request; every other failure arrives through the
    /// handle.
    fn start(&self, library: &str, function: &str, args: &[Json]) -> Result<u32, PluginError>;

    /// Block until one of `handles` settles and return which, with the call's
    /// own outcome. The outer `Err` is the wait itself being refused.
    #[allow(clippy::type_complexity)]
    fn wait_any(&self, handles: &[u32]) -> Result<(u32, Result<Json, PluginError>), PluginError>;

    fn script_log(&self, request: &ScriptLogRequest) -> Result<(), PluginError>;
    fn job_progress(&self, request: &JobProgressRequest) -> Result<(), PluginError>;
    fn job_should_stop(&self) -> Result<bool, PluginError>;
    fn job_wait(&self, seconds: f64) -> Result<(), PluginError>;
}

/// The host this module is instantiated by. Off wasm every wrapper answers
/// `NotWasm`, which is what a native test that reaches one sees.
pub struct Host;

impl Backend for Host {
    fn library_surface(&self, library: &str) -> Result<ApiSurface, PluginError> {
        host::library_surface(library)
    }

    fn start(&self, library: &str, function: &str, args: &[Json]) -> Result<u32, PluginError> {
        host::call_library_start(library, function, args)
    }

    fn wait_any(&self, handles: &[u32]) -> Result<(u32, Result<Json, PluginError>), PluginError> {
        host::call_library_wait_any(handles)
    }

    fn script_log(&self, request: &ScriptLogRequest) -> Result<(), PluginError> {
        host::script_log(request)
    }

    fn job_progress(&self, request: &JobProgressRequest) -> Result<(), PluginError> {
        host::job_progress(request)
    }

    fn job_should_stop(&self) -> Result<bool, PluginError> {
        host::job_should_stop()
    }

    fn job_wait(&self, seconds: f64) -> Result<(), PluginError> {
        host::job_wait(seconds)
    }
}

/// What `validate` runs against. Validation reads no run, so it has no
/// trigger to attribute a log line to and no job to report on: a `console`
/// call at file scope goes nowhere, and nothing else here is reachable,
/// because every import is a stub.
pub struct Validating;

impl Backend for Validating {
    fn library_surface(&self, library: &str) -> Result<ApiSurface, PluginError> {
        Err(not_validating(library))
    }

    fn start(&self, library: &str, _: &str, _: &[Json]) -> Result<u32, PluginError> {
        Err(not_validating(library))
    }

    fn wait_any(&self, _: &[u32]) -> Result<(u32, Result<Json, PluginError>), PluginError> {
        Err(not_validating("a call"))
    }

    fn script_log(&self, _: &ScriptLogRequest) -> Result<(), PluginError> {
        Ok(())
    }

    fn job_progress(&self, _: &JobProgressRequest) -> Result<(), PluginError> {
        Err(not_validating("a job"))
    }

    fn job_should_stop(&self) -> Result<bool, PluginError> {
        Err(not_validating("a job"))
    }

    fn job_wait(&self, _: f64) -> Result<(), PluginError> {
        Err(not_validating("a job"))
    }
}

fn not_validating(what: &str) -> PluginError {
    PluginError::host(format!("validation reaches no host, so not {what}"))
}

#[cfg(any(test, feature = "testing"))]
pub mod fake {
    //! A host that answers from the test: surfaces it was given, calls
    //! settled by a responder in an order the test picks, and every log line
    //! and job control recorded.

    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;

    use super::*;

    /// Which of the waited handles settles first.
    #[derive(Clone, Copy)]
    pub enum Order {
        /// The lowest handle, which is what the host answers when several
        /// have already settled.
        Lowest,
        /// The highest: the call started last finishes first.
        Highest,
    }

    type Responder = Box<dyn Fn(&str, &str, &[Json]) -> Result<Json, PluginError>>;

    /// A call as `(library, function, args)`.
    pub type Call = (String, String, Vec<Json>);

    pub struct Fake {
        surfaces: BTreeMap<String, ApiSurface>,
        responder: Responder,
        order: Cell<Order>,
        next: Cell<u32>,
        refuse_start: Cell<bool>,
        started: RefCell<BTreeMap<u32, Call>>,
        /// Every call, in the order started.
        pub calls: RefCell<Vec<Call>>,
        /// Every handle in the order it was settled.
        pub settled: RefCell<Vec<u32>>,
        pub logs: RefCell<Vec<ScriptLogRequest>>,
        pub progress: RefCell<Vec<Json>>,
        pub waits: RefCell<Vec<f64>>,
        pub should_stop: Cell<bool>,
    }

    impl Default for Fake {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Fake {
        pub fn new() -> Self {
            Self {
                surfaces: BTreeMap::new(),
                responder: Box::new(|_, _, args| Ok(Json::Array(args.to_vec()))),
                order: Cell::new(Order::Lowest),
                next: Cell::new(1),
                refuse_start: Cell::new(false),
                started: RefCell::new(BTreeMap::new()),
                calls: RefCell::new(Vec::new()),
                settled: RefCell::new(Vec::new()),
                logs: RefCell::new(Vec::new()),
                progress: RefCell::new(Vec::new()),
                waits: RefCell::new(Vec::new()),
                should_stop: Cell::new(false),
            }
        }

        pub fn surface(mut self, id: &str, surface: ApiSurface) -> Self {
            self.surfaces.insert(id.to_string(), surface);
            self
        }

        pub fn respond(
            mut self,
            responder: impl Fn(&str, &str, &[Json]) -> Result<Json, PluginError> + 'static,
        ) -> Self {
            self.responder = Box::new(responder);
            self
        }

        pub fn order(self, order: Order) -> Self {
            self.order.set(order);
            self
        }

        /// Make `start` itself fail, as it does when the host cannot read the
        /// request.
        pub fn refusing_start(self) -> Self {
            self.refuse_start.set(true);
            self
        }
    }

    impl Backend for Fake {
        fn library_surface(&self, library: &str) -> Result<ApiSurface, PluginError> {
            self.surfaces
                .get(library)
                .cloned()
                .ok_or_else(|| PluginError::new("LIBRARY_ERROR", format!("no library {library}")))
        }

        fn start(&self, library: &str, function: &str, args: &[Json]) -> Result<u32, PluginError> {
            if self.refuse_start.get() {
                return Err(PluginError::host("guest memory could not be read"));
            }
            let handle = self.next.get();
            self.next.set(handle + 1);
            let call = (library.to_string(), function.to_string(), args.to_vec());
            self.calls.borrow_mut().push(call.clone());
            self.started.borrow_mut().insert(handle, call);
            Ok(handle)
        }

        fn wait_any(
            &self,
            handles: &[u32],
        ) -> Result<(u32, Result<Json, PluginError>), PluginError> {
            let mut started = self.started.borrow_mut();
            let live = handles.iter().filter(|h| started.contains_key(h));
            let handle = match self.order.get() {
                Order::Lowest => live.min(),
                Order::Highest => live.max(),
            }
            .copied()
            .ok_or_else(|| PluginError::bad_input("no listed handle is in flight"))?;
            let (library, function, args) = started.remove(&handle).expect("filtered above");
            self.settled.borrow_mut().push(handle);
            Ok((handle, (self.responder)(&library, &function, &args)))
        }

        fn script_log(&self, request: &ScriptLogRequest) -> Result<(), PluginError> {
            self.logs.borrow_mut().push(request.clone());
            Ok(())
        }

        fn job_progress(&self, request: &JobProgressRequest) -> Result<(), PluginError> {
            self.progress.borrow_mut().push(request.data.clone());
            Ok(())
        }

        fn job_should_stop(&self) -> Result<bool, PluginError> {
            Ok(self.should_stop.get())
        }

        fn job_wait(&self, seconds: f64) -> Result<(), PluginError> {
            self.waits.borrow_mut().push(seconds);
            Ok(())
        }
    }
}

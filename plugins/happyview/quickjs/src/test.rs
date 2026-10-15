//! `internal.test` for a test file: a module written in JavaScript over a
//! handful of host bindings.

use std::cell::RefCell;
use std::rc::Rc;

use happyview_plugin_sdk::{TestReport, TestStatus};
use rquickjs::module::Declared;
use rquickjs::{Ctx, Function, Module, Object, Persistent, Result, Value};

use crate::backend::Backend;
use crate::builtins::throw_error;
use crate::imports::Project;

pub const MODULE: &str = "internal.test";

pub const NOT_A_TEST_FILE: &str =
    "module 'internal.test' is available only in test files (*.test.js, *.test.ts)";

const LIBRARY: &str = include_str!("test/library.js");

pub struct Suite {
    path: String,
    /// The run's project code, whose `Chunks` place a failure in the module
    /// it happened in and remap it through that module's source map.
    project: Rc<Project>,
    runner: RefCell<Option<Persistent<Function<'static>>>>,
}

impl Suite {
    pub fn new(path: &str, project: Rc<Project>) -> Rc<Self> {
        Rc::new(Self {
            path: path.to_string(),
            project,
            runner: RefCell::new(None),
        })
    }

    pub fn declare<'js>(
        self: &Rc<Self>,
        ctx: &Ctx<'js>,
        backend: &Rc<dyn Backend>,
    ) -> Result<Module<'js, Declared>> {
        let module = Module::declare(ctx.clone(), MODULE, LIBRARY)?;
        module.meta()?.set("host", self.bindings(ctx, backend)?)?;
        Ok(module)
    }

    /// The function the library registered, which runs every collected
    /// test. `None` when the file never imported the library.
    pub fn runner<'js>(&self, ctx: &Ctx<'js>) -> Result<Option<Function<'js>>> {
        match self.runner.borrow().as_ref() {
            Some(runner) => Ok(Some(runner.clone().restore(ctx)?)),
            None => Ok(None),
        }
    }

    /// Drop the runner. A value held across the run has to be released
    /// before the runtime is freed, which refuses to free one still live, and
    /// the suite itself is owned by the runtime's loader.
    pub fn release(&self) {
        self.runner.borrow_mut().take();
    }

    fn bindings<'js>(
        self: &Rc<Self>,
        ctx: &Ctx<'js>,
        backend: &Rc<dyn Backend>,
    ) -> Result<Object<'js>> {
        let host = Object::new(ctx.clone())?;

        let suite = self.clone();
        host.set(
            "register",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>, run: Function<'js>| -> Result<()> {
                    *suite.runner.borrow_mut() = Some(Persistent::save(&ctx, run));
                    Ok(())
                },
            )?,
        )?;

        let suite = self.clone();
        let reporter = backend.clone();
        host.set(
            "report",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>, report: Object<'js>| -> Result<()> {
                    let status: String = report.get("status")?;
                    let (status, message, file, line) = match status.as_str() {
                        "pass" => (TestStatus::Pass, None, None, None),
                        "skip" => (TestStatus::Skip, None, None, None),
                        _ => {
                            // Borrowed only for this call: the event loop adds a
                            // module to the run's chunks between tests.
                            let thrown = crate::describe_value(
                                &ctx,
                                report.get::<_, Value>("failure")?,
                                &suite.project.chunks.borrow(),
                            );
                            (
                                TestStatus::Fail,
                                Some(thrown.message),
                                thrown.chunk,
                                thrown.line,
                            )
                        }
                    };
                    let duration: f64 = report.get("durationMs")?;
                    reporter
                        .test_report(&TestReport {
                            path: suite.path.clone(),
                            name: report.get("name")?,
                            status,
                            message,
                            file,
                            line,
                            duration_ms: duration.max(0.0) as u64,
                        })
                        .map_err(|e| throw_error(&ctx, &format!("reporting a test: {}", e.message)))
                },
            )?,
        )?;

        Ok(host)
    }
}

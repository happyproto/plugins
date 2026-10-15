//! What a test file asks of the host, behind one trait, so a native test can
//! record it: off wasm every SDK wrapper answers `NotWasm`.

use happyview_plugin_sdk::{
    host, PluginError, TestCall, TestCalls, TestMock, TestPdsWrite, TestReport, TestTrigger,
    TestTriggerResult,
};

pub trait TestHost {
    fn trigger(&self, spec: &TestTrigger) -> Result<TestTriggerResult, PluginError>;
    fn mock(&self, spec: &TestMock) -> Result<(), PluginError>;
    fn calls(&self, spec: &TestCalls) -> Result<Vec<TestCall>, PluginError>;
    fn pds_writes(&self) -> Result<Vec<TestPdsWrite>, PluginError>;
    fn report(&self, report: &TestReport) -> Result<(), PluginError>;
}

pub struct Host;

impl TestHost for Host {
    fn trigger(&self, spec: &TestTrigger) -> Result<TestTriggerResult, PluginError> {
        host::test_trigger(spec)
    }
    fn mock(&self, spec: &TestMock) -> Result<(), PluginError> {
        host::test_mock(spec)
    }
    fn calls(&self, spec: &TestCalls) -> Result<Vec<TestCall>, PluginError> {
        host::test_calls(spec)
    }
    fn pds_writes(&self) -> Result<Vec<TestPdsWrite>, PluginError> {
        host::test_pds_writes()
    }
    fn report(&self, report: &TestReport) -> Result<(), PluginError> {
        host::test_report(report)
    }
}

/// What `validate` installs. Validation runs file scope to collect the tests
/// and runs none of them, so a trigger reached at file scope is the file's
/// mistake and says so.
pub struct Validating;

fn not_validating() -> PluginError {
    PluginError::host("validation runs no tests, so a trigger cannot fire at file scope")
}

impl TestHost for Validating {
    fn trigger(&self, _: &TestTrigger) -> Result<TestTriggerResult, PluginError> {
        Err(not_validating())
    }
    fn mock(&self, _: &TestMock) -> Result<(), PluginError> {
        Err(not_validating())
    }
    fn calls(&self, _: &TestCalls) -> Result<Vec<TestCall>, PluginError> {
        Err(not_validating())
    }
    fn pds_writes(&self) -> Result<Vec<TestPdsWrite>, PluginError> {
        Err(not_validating())
    }
    fn report(&self, _: &TestReport) -> Result<(), PluginError> {
        Ok(())
    }
}

#[cfg(test)]
pub use recording::Recording;

#[cfg(test)]
mod recording {
    use std::cell::{Cell, RefCell};

    use super::*;

    /// Records every request and answers triggers with `answer`, or refuses
    /// them when `refuse` is set.
    pub struct Recording {
        pub reports: RefCell<Vec<TestReport>>,
        pub triggers: RefCell<Vec<TestTrigger>>,
        pub mocks: RefCell<Vec<TestMock>>,
        pub answer: RefCell<TestTriggerResult>,
        pub refuse: Cell<bool>,
    }

    impl Default for Recording {
        fn default() -> Self {
            Self {
                reports: RefCell::new(Vec::new()),
                triggers: RefCell::new(Vec::new()),
                mocks: RefCell::new(Vec::new()),
                answer: RefCell::new(TestTriggerResult {
                    status: 200,
                    body: serde_json::json!({"thing": 1}),
                }),
                refuse: Cell::new(false),
            }
        }
    }

    impl TestHost for Recording {
        fn trigger(&self, spec: &TestTrigger) -> Result<TestTriggerResult, PluginError> {
            self.triggers.borrow_mut().push(spec.clone());
            if self.refuse.get() {
                return Err(PluginError::bad_input(
                    "com.example.unknown has no record lexicon in this project",
                ));
            }
            Ok(self.answer.borrow().clone())
        }
        fn mock(&self, spec: &TestMock) -> Result<(), PluginError> {
            self.mocks.borrow_mut().push(spec.clone());
            Ok(())
        }
        fn calls(&self, _: &TestCalls) -> Result<Vec<TestCall>, PluginError> {
            Ok(vec![TestCall {
                method: "get".into(),
                args: serde_json::json!(["https://example.com"]),
            }])
        }
        fn pds_writes(&self) -> Result<Vec<TestPdsWrite>, PluginError> {
            Ok(Vec::new())
        }
        fn report(&self, report: &TestReport) -> Result<(), PluginError> {
            self.reports.borrow_mut().push(report.clone());
            Ok(())
        }
    }
}

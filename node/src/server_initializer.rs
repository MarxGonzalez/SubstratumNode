// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.
use super::bootstrapper::Bootstrapper;
use super::entry_dns::new_dns_socket_server;
use super::privilege_drop::PrivilegeDropper;
use super::privilege_drop::PrivilegeDropperReal;
use crate::sub_lib::main_tools::Command;
use crate::sub_lib::main_tools::StdStreams;
use crate::sub_lib::socket_server::SocketServer;
use flexi_logger::Duplicate;
use flexi_logger::LevelFilter;
use flexi_logger::LogSpecification;
use flexi_logger::Logger;
use futures::try_ready;
use std::env::temp_dir;
use tokio::prelude::Async;
use tokio::prelude::Future;

pub struct ServerInitializer<P>
where
    P: PrivilegeDropper,
{
    dns_socket_server: Box<dyn SocketServer<Item = (), Error = ()>>,
    bootstrapper: Box<dyn SocketServer<Item = (), Error = ()>>,
    privilege_dropper: P,
    logger_initializer_wrapper: Box<dyn LoggerInitializerWrapper>,
}

impl<P> Command for ServerInitializer<P>
where
    P: PrivilegeDropper,
{
    fn go(&mut self, streams: &mut StdStreams<'_>, args: &Vec<String>) -> u8 {
        self.dns_socket_server
            .as_mut()
            .initialize_as_privileged(args, &mut self.logger_initializer_wrapper);
        self.bootstrapper
            .as_mut()
            .initialize_as_privileged(args, &mut self.logger_initializer_wrapper);

        self.privilege_dropper.drop_privileges();

        self.dns_socket_server
            .as_mut()
            .initialize_as_unprivileged(streams);
        self.bootstrapper
            .as_mut()
            .initialize_as_unprivileged(streams);

        1
    }
}

impl<P> Future for ServerInitializer<P>
where
    P: PrivilegeDropper,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Result<Async<<Self as Future>::Item>, <Self as Future>::Error> {
        try_ready!(self
            .dns_socket_server
            .as_mut()
            .join(self.bootstrapper.as_mut())
            .poll());
        Ok(Async::Ready(()))
    }
}

impl ServerInitializer<PrivilegeDropperReal> {
    pub fn new() -> ServerInitializer<PrivilegeDropperReal> {
        ServerInitializer {
            dns_socket_server: Box::new(new_dns_socket_server()),
            bootstrapper: Box::new(Bootstrapper::new()),
            privilege_dropper: PrivilegeDropperReal::new(),
            logger_initializer_wrapper: Box::new(LoggerInitializerWrapperReal {}),
        }
    }
}

pub trait LoggerInitializerWrapper: Send {
    fn init(&mut self, log_level: LevelFilter) -> bool;
}

struct LoggerInitializerWrapperReal {}

impl LoggerInitializerWrapper for LoggerInitializerWrapperReal {
    fn init(&mut self, log_level: LevelFilter) -> bool {
        match Logger::with(LogSpecification::default(log_level).finalize())
            .log_to_file()
            .directory(&temp_dir().to_str().expect("Bad temporary filename")[..])
            .print_message()
            .duplicate_to_stderr(Duplicate::Info)
            .suppress_timestamp()
            .start()
        {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}

#[cfg(test)]
pub mod test_utils {
    use crate::privilege_drop::PrivilegeDropper;
    use crate::server_initializer::LoggerInitializerWrapper;
    use crate::test_utils::logging::init_test_logging;
    use log::LevelFilter;
    use std::sync::{Arc, Mutex};

    pub struct PrivilegeDropperMock {
        pub call_count: Arc<Mutex<usize>>,
    }

    impl PrivilegeDropperMock {
        pub fn new() -> PrivilegeDropperMock {
            PrivilegeDropperMock {
                call_count: Arc::new(Mutex::new(0)),
            }
        }
    }

    impl PrivilegeDropper for PrivilegeDropperMock {
        fn drop_privileges(&self) {
            let mut calls = self.call_count.lock().unwrap();
            *calls += 1;
        }
    }

    pub struct LoggerInitializerWrapperMock {
        init_parameters: Arc<Mutex<Vec<LevelFilter>>>,
    }

    impl LoggerInitializerWrapper for LoggerInitializerWrapperMock {
        fn init(&mut self, log_level: LevelFilter) -> bool {
            self.init_parameters.lock().unwrap().push(log_level);
            init_test_logging()
        }
    }

    impl LoggerInitializerWrapperMock {
        pub fn new() -> LoggerInitializerWrapperMock {
            LoggerInitializerWrapperMock {
                init_parameters: Arc::new(Mutex::new(vec![])),
            }
        }

        pub fn init_parameters(&mut self, parameters: &Arc<Mutex<Vec<LevelFilter>>>) {
            self.init_parameters = parameters.clone();
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::crash_test_dummy::CrashTestDummy;
    use crate::server_initializer::test_utils::{
        LoggerInitializerWrapperMock, PrivilegeDropperMock,
    };
    use crate::sub_lib::crash_point::CrashPoint;
    use crate::test_utils::test_utils::ByteArrayReader;
    use crate::test_utils::test_utils::ByteArrayWriter;
    use std::sync::Arc;
    use std::sync::Mutex;

    impl SocketServer for CrashTestDummy {
        fn name(&self) -> String {
            String::from("crash test SocketServer")
        }

        fn initialize_as_privileged(
            &mut self,
            _args: &Vec<String>,
            _logger_initializer: &mut Box<dyn LoggerInitializerWrapper>,
        ) {
        }

        fn initialize_as_unprivileged(&mut self, _streams: &mut StdStreams<'_>) {}
    }

    #[test]
    fn exits_after_all_socket_servers_exit() {
        let dns_socket_server = CrashTestDummy::new(CrashPoint::Error);
        let bootstrapper = CrashTestDummy::new(CrashPoint::Error);

        let privilege_dropper = PrivilegeDropperMock::new();
        let mut logger_initializer_wrapper_mock = LoggerInitializerWrapperMock::new();
        let logger_init_parameters: Arc<Mutex<Vec<LevelFilter>>> = Arc::new(Mutex::new(vec![]));
        logger_initializer_wrapper_mock.init_parameters(&logger_init_parameters);

        let mut subject = ServerInitializer {
            dns_socket_server: Box::new(dns_socket_server),
            bootstrapper: Box::new(bootstrapper),
            privilege_dropper,
            logger_initializer_wrapper: Box::new(logger_initializer_wrapper_mock),
        };

        let stdin = &mut ByteArrayReader::new(&[0; 0]);
        let stdout = &mut ByteArrayWriter::new();
        let stderr = &mut ByteArrayWriter::new();
        let streams = &mut StdStreams {
            stdin,
            stdout,
            stderr,
        };

        let args = vec![String::from("glorp")];

        subject.go(streams, &args);
        let res = subject.wait();

        assert!(res.is_err());
    }

    #[test]
    fn server_initializer_as_a_future() {
        let dns_socket_server = CrashTestDummy::new(CrashPoint::None);
        let bootstrapper = CrashTestDummy::new(CrashPoint::None);
        let privilege_dropper = PrivilegeDropperMock::new();

        let mut subject = ServerInitializer {
            dns_socket_server: Box::new(dns_socket_server),
            bootstrapper: Box::new(bootstrapper),
            privilege_dropper,
            logger_initializer_wrapper: Box::new(LoggerInitializerWrapperMock::new()),
        };

        let result = subject.poll();
        assert_eq!(result, Ok(Async::Ready(())))
    }

    #[test]
    #[should_panic(expected = "EntryDnsServerMock was instructed to panic")]
    fn server_initializer_dns_socket_server_panics() {
        let bootstrapper = CrashTestDummy::new(CrashPoint::None);
        let privilege_dropper = PrivilegeDropperMock::new();

        let mut subject = ServerInitializer {
            dns_socket_server: Box::new(CrashTestDummy::panic(
                "EntryDnsServerMock was instructed to panic".to_string(),
            )),
            bootstrapper: Box::new(bootstrapper),
            privilege_dropper,
            logger_initializer_wrapper: Box::new(LoggerInitializerWrapperMock::new()),
        };

        let _ = subject.poll();
    }

    #[test]
    #[should_panic(expected = "BootstrapperMock was instructed to panic")]
    fn server_initializer_bootstrapper_panics() {
        let dns_socket_server = CrashTestDummy::new(CrashPoint::None);
        let privilege_dropper = PrivilegeDropperMock::new();
        let mut subject = ServerInitializer {
            dns_socket_server: Box::new(dns_socket_server),
            bootstrapper: Box::new(CrashTestDummy::panic(
                "BootstrapperMock was instructed to panic".to_string(),
            )),
            privilege_dropper,
            logger_initializer_wrapper: Box::new(LoggerInitializerWrapperMock::new()),
        };

        let _ = subject.poll();
    }

    #[test]
    fn go_should_drop_privileges() {
        let bootstrapper = CrashTestDummy::new(CrashPoint::None);
        let privilege_dropper = PrivilegeDropperMock::new();

        let call_count = Arc::clone(&privilege_dropper.call_count);

        let stdin = &mut ByteArrayReader::new(&[0; 0]);
        let stdout = &mut ByteArrayWriter::new();
        let stderr = &mut ByteArrayWriter::new();
        let streams = &mut StdStreams {
            stdin,
            stdout,
            stderr,
        };
        let mut subject = ServerInitializer {
            dns_socket_server: Box::new(CrashTestDummy::new(CrashPoint::None)),
            bootstrapper: Box::new(bootstrapper),
            privilege_dropper,
            logger_initializer_wrapper: Box::new(LoggerInitializerWrapperMock::new()),
        };

        subject.go(streams, &vec![]);

        assert_eq!(*call_count.lock().unwrap(), 1);
    }
}

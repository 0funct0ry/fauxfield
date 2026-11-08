#[cfg(unix)]
mod unix {
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    use fauxfield_control::{
        ControlApi, HostLock, LocalControl, RemoteControl, ipc, read_host, serve,
    };
    use fauxfield_engine::{Engine, EngineConfig, TestRequest, TrafficFilter};
    use fauxfield_model::parse_server;

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn server(port: u16) -> fauxfield_model::Server {
        parse_server(&format!(
            r#"
id = "srv_a"
name = "A"
port = {port}
[[routes]]
id = "rte_1"
path = "/hi"
[[routes.responses]]
id = "rsp_1"
body = "hello"
"#
        ))
        .unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn json_rpc_round_trip_over_a_0600_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("c.sock");
        let engine = Engine::new(tokio::runtime::Handle::current(), EngineConfig::default());
        engine.apply(vec![server(free_port())]);
        let local = LocalControl::new(engine);
        let (listener, guard) = ipc::bind_unix(&sock, "test", "x").await.unwrap();
        assert_eq!(
            std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777,
            0o600
        );
        tokio::spawn(serve(listener, Arc::new(local)));

        let c = RemoteControl::new(&sock);
        assert_eq!(c.status().await.unwrap()[0].status.label(), "stopped");
        assert_eq!(c.start(None).await.unwrap()[0].status.label(), "running");
        let r = c
            .send_test(
                "srv_a".into(),
                TestRequest {
                    method: "GET".into(),
                    path: "/hi".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!((r.status, r.body.as_str()), (200, "hello"));
        let t = c.traffic(TrafficFilter::default()).await.unwrap();
        assert_eq!(t.len(), 1);
        assert!(
            c.traffic(TrafficFilter {
                after_id: Some(t[0].id),
                ..Default::default()
            })
            .await
            .unwrap()
            .is_empty()
        );
        let e = c.start(Some("nope".into())).await.unwrap_err();
        assert_eq!(e.kind, "unknown_server");
        c.stop(None).await.unwrap();
        drop(guard);
        assert!(!sock.exists());
    }

    #[tokio::test]
    async fn stale_socket_is_replaced_and_live_one_refused() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("s.sock");
        let (l, g) = ipc::bind_unix(&sock, "test", "x").await.unwrap();
        assert!(ipc::bind_unix(&sock, "test", "x").await.is_err());
        drop(l);
        std::mem::forget(g); // leave the file behind: stale
        assert!(sock.exists());
        assert!(ipc::bind_unix(&sock, "test", "x").await.is_ok());
    }

    #[tokio::test]
    async fn host_lock_detects_live_and_stale_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sock = root.join("h.sock");
        assert!(read_host(root).is_none());
        let (listener, _g) = ipc::bind_unix(&sock, "test", "x").await.unwrap();
        let lock = HostLock::acquire(root, &sock, "now".into()).unwrap();
        assert_eq!(read_host(root).unwrap().pid, std::process::id());
        assert!(HostLock::acquire(root, &sock, "now".into()).is_err());
        drop(lock);
        assert!(read_host(root).is_none());
        // A host.json whose socket is dead is stale.
        let _l2 = HostLock::acquire(root, &sock, "now".into()).unwrap();
        drop(listener);
        assert!(read_host(root).is_none());
    }
}

#[cfg(windows)]
mod windows {
    use std::net::TcpListener;
    use std::sync::Arc;

    use fauxfield_control::{
        ControlApi, HostLock, LocalControl, RemoteControl, control_socket_path, ipc, read_host,
        serve,
    };
    use fauxfield_engine::{Engine, EngineConfig};
    use fauxfield_model::parse_server;

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn json_rpc_round_trip_over_a_named_pipe() {
        let dir = tempfile::tempdir().unwrap();
        let name = control_socket_path(dir.path());
        assert!(name.to_string_lossy().starts_with(r"\\.\pipe\fauxfield-"));
        let engine = Engine::new(tokio::runtime::Handle::current(), EngineConfig::default());
        let s = parse_server(&format!(
            "id = \"srv_a\"\nname = \"A\"\nport = {}\n",
            free_port()
        ))
        .unwrap();
        engine.apply(vec![s]);
        let first = ipc::bind_pipe(&name.to_string_lossy()).unwrap();
        // A squatter cannot create the same name.
        assert!(ipc::bind_pipe(&name.to_string_lossy()).is_err());
        tokio::spawn(serve(
            first,
            name.to_string_lossy().to_string(),
            Arc::new(LocalControl::new(engine)),
        ));
        let c = RemoteControl::new(&name);
        assert_eq!(c.status().await.unwrap()[0].status.label(), "stopped");
        assert_eq!(c.start(None).await.unwrap()[0].status.label(), "running");
        // Many sequential clients are served (instances are re-created).
        for _ in 0..5 {
            c.status().await.unwrap();
        }
        c.stop(None).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_lock_detects_live_and_stale_pipes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let name = control_socket_path(root);
        assert!(read_host(root).is_none());
        let first = ipc::bind_pipe(&name.to_string_lossy()).unwrap();
        let api = Arc::new(LocalControl::new(Engine::new(
            tokio::runtime::Handle::current(),
            EngineConfig::default(),
        )));
        let task = tokio::spawn(serve(first, name.to_string_lossy().to_string(), api));
        let lock = HostLock::acquire(root, &name, "now".into()).unwrap();
        assert!(read_host(root).is_some());
        assert!(HostLock::acquire(root, &name, "now".into()).is_err());
        drop(lock);
        task.abort();
    }
}

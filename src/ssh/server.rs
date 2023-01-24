use russh::server;
use russh::server::Config;
use russh::MethodSet;
use russh_keys::key::KeyPair;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::spawn;
use tokio::sync::mpsc::Sender;
use tokio::sync::watch;

use super::handler::ThinHandler;
use super::session_manager::SessionManager;
use super::session_manager::SessionRepoUpdate;

pub struct Server {
    pub listen: IpAddr,
    pub port: u16,
    pub server_keys: Vec<KeyPair>,
    pub connection_timeout: Duration,
    pub auth_rejection_time: Duration,
    pub exit_rx: watch::Receiver<bool>,
    pub stdio_lock: Arc<Mutex<()>>,
    session_sender: Sender<SessionRepoUpdate>,
}

impl Server {
    pub async fn new(
        server_keys: &[KeyPair],
        rx_exit: watch::Receiver<bool>,
        sender: Sender<SessionRepoUpdate>,
        port: u16,
    ) -> Self {
        Self {
            server_keys: server_keys.to_vec(),
            connection_timeout: Duration::from_secs(600),
            auth_rejection_time: Duration::from_secs(0),
            exit_rx: rx_exit,
            stdio_lock: Arc::new(Mutex::new(())),
            listen: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port,
            session_sender: sender,
        }
    }
    pub async fn listen(
        self,
        mut session_repository: SessionManager,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::default();
        config.connection_timeout = Some(self.connection_timeout.clone());
        config.auth_rejection_time = self.auth_rejection_time.clone();
        for key in &self.server_keys {
            config.keys.push(key.clone().into());
        }
        config.methods = MethodSet::PUBLICKEY;

        let config = Arc::new(config);

        let addr = format!("{}:{}", self.listen, self.port);

        println!("Listening on {}", addr);

        spawn(async move {
            session_repository.wait_for_sessions().await;
        });

        russh::server::run(
            config,
            &SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), self.port)),
            self,
        )
        .await?;
        Ok(())
    }
}

impl server::Server for Server {
    type Handler = ThinHandler;

    fn new_client(&mut self, _peer_addr: Option<SocketAddr>) -> Self::Handler {
        ThinHandler::new(self.session_sender.clone())
    }
}

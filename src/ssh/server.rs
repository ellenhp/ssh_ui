use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use thrussh::server;
use thrussh::MethodSet;
use tokio::spawn;
use tokio::sync::mpsc::Sender;
use tokio::sync::watch;

use super::handler::ThinHandler;
#[allow(unused_imports)]
use super::key::SshServerKey;
use super::session_manager::SessionManager;
use super::session_manager::SessionRepoUpdate;

pub struct Server {
    pub listen: IpAddr,
    pub port: u16,
    pub server_key: SshServerKey,
    pub connection_timeout: Duration,
    pub auth_rejection_time: Duration,
    pub exit_rx: watch::Receiver<bool>,
    pub stdio_lock: Arc<Mutex<()>>,
    session_sender: Sender<SessionRepoUpdate>,
}

impl Server {
    pub async fn new(
        server_key: SshServerKey,
        rx_exit: watch::Receiver<bool>,
        sender: Sender<SessionRepoUpdate>,
        port: u16,
    ) -> Self {
        Self {
            server_key,
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
        let mut config = thrussh::server::Config::default();
        config.connection_timeout = Some(self.connection_timeout.clone());
        config.auth_rejection_time = self.auth_rejection_time.clone();
        config.keys.push(self.server_key.clone().into());
        config.methods = MethodSet::PUBLICKEY;

        let config = Arc::new(config);

        let addr = format!("{}:{}", self.listen, self.port);

        println!("Listening on {}", addr);

        spawn(async move {
            session_repository.wait_for_sessions().await;
        });

        thrussh::server::run(config, addr.as_str(), self).await?;
        Ok(())
    }
}

impl server::Server for Server {
    type Handler = ThinHandler;

    // Called for each new connection.
    fn new(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> ThinHandler {
        ThinHandler::new(self.session_sender.clone())
    }
}

use log::info;
use log::trace;
use russh::keys::PrivateKey;
use russh::server;
use russh::server::Config;
use russh::server::Server as _;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::sync::Arc;
use std::time::Duration;
use tokio::spawn;
use tokio::sync::mpsc::Sender;

use super::handler::ThinHandler;
use super::session_manager::SessionManager;
use super::session_manager::SessionRepoUpdate;

pub struct Server {
    pub listen: IpAddr,
    pub port: u16,
    pub server_keys: Vec<PrivateKey>,
    pub auth_rejection_time: Duration,
    session_sender: Sender<SessionRepoUpdate>,
}

impl Server {
    pub async fn new(
        server_keys: &[PrivateKey],
        sender: Sender<SessionRepoUpdate>,
        port: u16,
    ) -> Self {
        Self {
            server_keys: server_keys.to_vec(),
            auth_rejection_time: Duration::from_secs(0),
            listen: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port,
            session_sender: sender,
        }
    }
    pub async fn listen(
        mut self,
        mut session_repository: SessionManager,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = Config::default();
        config.auth_rejection_time = self.auth_rejection_time.clone();
        for key in &self.server_keys {
            config.keys.push(key.clone().into());
        }

        let config = Arc::new(config);

        let addr = format!("{}:{}", self.listen, self.port);

        info!("Listening on {}", addr);

        spawn(async move {
            session_repository.wait_for_sessions().await;
        });

        let socket = tokio::net::TcpSocket::new_v4()?;
        socket.bind(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(0, 0, 0, 0),
            self.port,
        )))?;
        let listener = socket.listen(1024)?;

        self.run_on_socket(config, &listener).await?;
        Ok(())
    }
}

impl server::Server for Server {
    type Handler = ThinHandler;

    fn new_client(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        trace!("New client created for peer {:?}", peer_addr);
        ThinHandler::new(self.session_sender.clone())
    }
}

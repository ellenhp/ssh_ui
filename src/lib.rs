pub(crate) mod ssh;

#[macro_use]
extern crate lazy_static;

use std::{error::Error, sync::Arc};

use cursive::View;

pub use cursive;
pub use russh_keys;

use russh_keys::key::KeyPair;
use ssh::{plugin::set_plugin, server::Server, session_manager::SessionManager};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionHandle(u64);

pub trait AppSession {
    /// Called when the session starts. Returns a cursive view that will be displayed to the user.
    fn on_start(
        &mut self,
        siv: &mut cursive::Cursive,
        session_handle: SessionHandle,
        pub_key: russh_keys::key::PublicKey,
    ) -> Result<Box<dyn View>, Box<dyn Error>>;

    /// Called when the session ticks.
    fn on_tick(&mut self, _siv: &mut cursive::Cursive) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

/// A plugin that lets you integrate with the ssh_ui system.
pub trait App: Send + Sync {
    /// Called when the plugin is loaded.
    fn on_load(&mut self) -> Result<(), Box<dyn Error>>;
    /// Called to request a new session.
    fn new_session(&self) -> Box<dyn AppSession>;
}

/// Server that handles incoming ssh connections.
pub struct AppServer {
    port: u16,
}

impl AppServer {
    /// Creates a new server with the specified port.
    pub fn new_with_port(port: u16) -> Self {
        Self { port }
    }

    /// Listens on the specified port for new ssh connections indefinitely.
    pub async fn run(
        &mut self,
        key_pairs: &[KeyPair],
        plugin: Arc<dyn App>,
    ) -> Result<(), Box<dyn Error>> {
        set_plugin(plugin);
        let (sender, receiver) = mpsc::channel(100);
        let (_tx, rx) = watch::channel(false);
        let repo = SessionManager::new(sender.clone(), receiver);
        let sh = Server::new(key_pairs, rx, sender, self.port).await;
        sh.listen(repo).await.unwrap();

        Ok(())
    }
}

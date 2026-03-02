use lazy_static::lazy_static;
use russh::keys::PublicKey;
use std::fs::File;
use std::sync::Arc;

use crate::cursive::Cursive;
use crate::cursive::Vec2;
use crate::{App, SessionHandle};

use cursive::event::Event;
use log::trace;
use tokio::sync::mpsc::channel;

use super::backend::{Backend, CursiveOutput};

lazy_static! {
    static ref PLUGINS: tokio::sync::Mutex<Option<Arc<dyn App>>> = tokio::sync::Mutex::new(None);
}

pub(super) async fn get_plugin() -> Option<Arc<dyn App>> {
    let plugins_tmp = PLUGINS.lock().await;
    plugins_tmp.as_ref().cloned()
}

pub async fn set_plugin(plugin: Arc<dyn App>) {
    let mut plugins_tmp = PLUGINS.lock().await;
    plugins_tmp.replace(plugin);
}

pub struct PluginManager {
    bbs_side_input: File,
    output_sender: tokio::sync::mpsc::Sender<CursiveOutput>,
    resize_receiver: tokio::sync::mpsc::Receiver<Vec2>,
    relayout_sender: tokio::sync::mpsc::Sender<()>,
    relayout_receiver: tokio::sync::mpsc::Receiver<()>,
}

unsafe impl Send for PluginManager {}

impl PluginManager {
    pub fn new(
        bbs_side_input: File,
        output_sender: tokio::sync::mpsc::Sender<CursiveOutput>,
        resize_receiver: tokio::sync::mpsc::Receiver<Vec2>,
        relayout_sender: tokio::sync::mpsc::Sender<()>,
        relayout_receiver: tokio::sync::mpsc::Receiver<()>,
    ) -> Self {
        Self {
            bbs_side_input,
            output_sender,
            resize_receiver,
            relayout_sender,
            relayout_receiver,
        }
    }

    pub async fn event_loop(
        mut self,
        pub_key: Option<PublicKey>,
        handle_id: SessionHandle,
        mut exit_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        trace!("Entering event loop for session handle {}", handle_id.0);

        let (client_facing_relayout_sender, mut client_facing_relayout_receiver) = channel(10);

        let mut session = get_plugin()
            .await
            .expect("Missing plugin")
            .as_ref()
            .new_session();
        let mut siv = Cursive::new();
        let view = session.on_start(&mut siv, handle_id, pub_key, client_facing_relayout_sender)?;
        siv.add_layer(view);

        let backend = Backend::init_ssh(
            self.bbs_side_input,
            self.output_sender,
            self.resize_receiver,
            self.relayout_sender,
        )
        .expect("Russh backend creation failed");

        {
            let mut runner = siv.runner(backend);
            runner.add_global_callback(Event::Refresh, move |siv| {
                let _ = session.on_tick(siv);
            });

            runner.refresh();
            runner.on_event(Event::Refresh);
            runner.step();
            while runner.is_running() && !*exit_rx.borrow_and_update() {
                if self.relayout_receiver.try_recv().is_ok()
                    || client_facing_relayout_receiver.try_recv().is_ok()
                {
                    trace!("Forcefully refreshing layout for session {}", handle_id.0);
                    // TODO: Figure out why this is necessary. It seems like we do actually need two refreshes and a step to make this work :(
                    runner.refresh();
                    runner.on_event(Event::Refresh);
                    runner.step();
                    runner.refresh();
                    runner.on_event(Event::Refresh);
                } else {
                    runner.step();
                }
            }
        }
        trace!("Exiting event loop for session {}", handle_id.0);
        Ok(())
    }
}

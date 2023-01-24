use std::fs::File;
use std::sync::{Arc, Mutex};

use crate::cursive::Cursive;
use crate::cursive::Vec2;
use crate::{App, SessionHandle};

use cursive::event::Event;
use russh_keys::key::PublicKey;
use tokio::runtime::Builder;

use super::backend::{Backend, CursiveOutput};

lazy_static! {
    static ref PLUGINS: Mutex<Option<Arc<dyn App>>> = Mutex::new(None);
}

pub(super) fn get_plugin() -> Option<Arc<dyn App>> {
    let plugins_tmp = PLUGINS.lock().unwrap();
    plugins_tmp.as_ref().cloned()
}

pub fn set_plugin(plugin: Arc<dyn App>) {
    let mut plugins_tmp = PLUGINS.lock().unwrap();
    plugins_tmp.replace(plugin);
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct PluginId(usize);

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

    pub fn event_loop(
        mut self,
        pub_key: PublicKey,
        handle_id: SessionHandle,
        mut exit_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut siv = Cursive::new();

        let plugin = get_plugin().unwrap();
        let mut session = plugin.as_ref().new_session();
        let view = session.on_start(&mut siv, handle_id, pub_key)?;
        siv.add_layer(view);

        let backend = Backend::init_ssh(
            self.bbs_side_input,
            self.output_sender,
            self.resize_receiver,
            self.relayout_sender,
        )
        .expect("Russh backend creation failed");

        let runtime = Builder::new_multi_thread().worker_threads(2).build()?;
        let _enter = runtime.handle().enter();

        {
            let mut runner = siv.runner(backend);
            runner.add_global_callback(Event::Refresh, move |siv| {
                let _ = session.on_tick(siv);
            });

            runner.refresh();
            runner.on_event(Event::Refresh);
            let mut counter = 0usize;
            while runner.is_running() && !*exit_rx.borrow_and_update() {
                runner.step();
                if counter % 10 == 0 || self.relayout_receiver.try_recv().is_ok() {
                    runner.refresh();
                    runner.on_event(Event::Refresh);
                }
                counter += 1;
            }
        }
        Ok(())
    }
}

use std::{fmt::Debug, fs::File, time::Duration};

use crate::{cursive::Vec2, SessionHandle};
use async_std::io::WriteExt;
use log::{debug, info};
use russh::{server::Handle, ChannelId, CryptoVec};
use russh_keys::key::PublicKey;
use tokio::{
    spawn,
    sync::{
        mpsc::{channel, Receiver, Sender},
        watch,
    },
    time::sleep,
};

use crate::ssh::plugin::PluginManager;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SshSessionUpdate {
    Data(Vec<u8>),
    WindowResize(usize, usize),
    Close,
}

pub enum SessionRepoUpdate {
    NewSession(
        Handle,
        ChannelId,
        Receiver<SshSessionUpdate>,
        Option<PublicKey>,
    ),
}

impl Debug for SessionRepoUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewSession(_arg0, arg1, arg2, arg3) => f
                .debug_tuple("NewSession")
                .field(arg1)
                .field(arg2)
                .field(arg3)
                .finish(),
        }
    }
}

pub struct SessionManager {
    pub update_sender: Sender<SessionRepoUpdate>,
    pub update_receiver: Receiver<SessionRepoUpdate>,
}

impl SessionManager {
    pub fn new(
        update_sender: Sender<SessionRepoUpdate>,
        update_receiver: Receiver<SessionRepoUpdate>,
    ) -> Self {
        Self {
            update_sender,
            update_receiver,
        }
    }

    pub async fn wait_for_sessions(&mut self) {
        let mut handle_cursor = 0u64;
        loop {
            let update = self.update_receiver.recv().await;
            if update.is_none() {
                continue;
            }
            match update.unwrap() {
                SessionRepoUpdate::NewSession(handle, channel_id, update_rx, key) => {
                    let handle_id = handle_cursor.clone();
                    handle_cursor += 1;
                    spawn(async move {
                        Self::handle_session(
                            handle,
                            channel_id,
                            update_rx,
                            SessionHandle(handle_id),
                            key,
                        )
                        .await;
                    });
                }
            }
        }
    }

    async fn handle_session(
        handle: Handle,
        channel_id: ChannelId,
        mut update_rx: Receiver<SshSessionUpdate>,
        handle_id: SessionHandle,
        key: Option<PublicKey>,
    ) {
        info!("Handling new session {}", handle_id.0);
        let (mut ssh_side_output, bbs_side_input): (async_std::fs::File, File) = {
            let (bbs_side, ssh_side, _name) =
                openpty::openpty(None, None, None).expect("Creating pty failed");
            (ssh_side.into(), bbs_side)
        };
        let (output_sender, mut output_receiver) = channel(100000);
        let (resize_sender, resize_receiver) = channel(100);
        let (exit_tx, exit_rx) = watch::channel(false);
        let (relayout_sender, relayout_receiver) = channel(100);

        let plugin_manager = PluginManager::new(
            bbs_side_input,
            output_sender,
            resize_receiver,
            relayout_sender,
            relayout_receiver,
        );

        let handle_id = handle_id.clone();
        let join_handle = std::thread::spawn(move || {
            debug!("Starting event loop thread for session: {}", handle_id.0);
            plugin_manager.event_loop(key, handle_id, exit_rx).unwrap();
            debug!(
                "Falling out of event loop thread for session: {}",
                handle_id.0
            );
        });
        let forwarding_task_handle = spawn(async move {
            debug!(
                "Entering output forwarding task for session: {}",
                handle_id.0
            );
            loop {
                match output_receiver.recv().await {
                    Some(output) => match output {
                        crate::ssh::backend::CursiveOutput::Data(data) => {
                            handle
                                .data(channel_id, CryptoVec::from_slice(&data))
                                .await
                                .unwrap();
                        }
                        crate::ssh::backend::CursiveOutput::Close => {
                            debug!(
                                "Output forwarding task found close event on session: {}",
                                handle_id.0
                            );
                            handle.close(channel_id).await.unwrap();
                            break;
                        }
                    },
                    None => {
                        sleep(Duration::from_millis(1)).await;
                    }
                }
            }
            debug!(
                "Falling through output forwarding task for session: {}",
                handle_id.0
            );
        });
        spawn(async move {
            debug!(
                "Entering input forwarding task for session: {}",
                handle_id.0
            );
            loop {
                let update = update_rx.recv().await;
                if update.is_none() {
                    sleep(Duration::from_millis(1)).await;
                    continue;
                }
                match update.unwrap() {
                    SshSessionUpdate::Data(data) => {
                        ssh_side_output.write_all(&data).await.unwrap();
                        ssh_side_output.flush().await.unwrap();
                    }
                    SshSessionUpdate::WindowResize(width, height) => {
                        resize_sender.send(Vec2::new(width, height)).await.unwrap();
                    }
                    SshSessionUpdate::Close => {
                        debug!(
                            "Found close event on input forwarding task for session: {}",
                            handle_id.0
                        );
                    }
                }
            }
        })
        .await
        .unwrap();
        debug!("Fell through input forwarding task, indicating disconnection on session {}. Aborting/joining other tasks/threads.", handle_id.0);
        let _ = exit_tx.send(true);
        forwarding_task_handle.abort();
        join_handle.join().expect("Failed to join thread");
        info!("Cleaned up from disconnected session: {}", handle_id.0);
    }
}

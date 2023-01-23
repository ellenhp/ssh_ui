use std::{fmt::Debug, fs::File, time::Duration};

use crate::cursive::Vec2;
use async_std::io::WriteExt;
use thrussh::{server::Handle, ChannelId, CryptoVec};
use thrussh_keys::key::PublicKey;
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
    NewSession(Handle, ChannelId, Receiver<SshSessionUpdate>, PublicKey),
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
        loop {
            let update = self.update_receiver.recv().await;
            if update.is_none() {
                continue;
            }
            match update.unwrap() {
                SessionRepoUpdate::NewSession(handle, channel_id, update_rx, key) => {
                    spawn(async move {
                        Self::handle_session(handle, channel_id, update_rx, key).await;
                    });
                }
            }
        }
    }

    async fn handle_session(
        mut handle: Handle,
        channel_id: ChannelId,
        mut update_rx: Receiver<SshSessionUpdate>,
        key: PublicKey,
    ) {
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

        let join_handle = std::thread::spawn(move || {
            plugin_manager.event_loop(key, exit_rx).unwrap();
        });
        let forwarding_task_handle = spawn(async move {
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
                            handle.close(channel_id).await.unwrap();
                        }
                    },
                    None => {
                        sleep(Duration::from_millis(1)).await;
                    }
                }
            }
        });
        spawn(async move {
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
                    SshSessionUpdate::Close => return,
                }
            }
        })
        .await
        .unwrap();
        let _ = exit_tx.send(true);
        forwarding_task_handle.abort();
        join_handle.join().expect("Failed to join thread");
    }
}

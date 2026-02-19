use nix::sys::socket::{self, ControlMessageOwned, MultiHeaders, UnixAddr};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net;

pub struct SocketRawParsedMessage {
    data: Vec<u8>,
    fds: Vec<i32>,
}

impl SocketRawParsedMessage {
    pub fn read_many_from_socket(socket: &net::UnixStream) -> nix::Result<Vec<Self>> {
        const BUFFER_SIZE: usize = 8192;
        const MAX_MESSAGES: usize = 32;
        const MAX_FDS_PER_MSG: usize = 255;

        let mut receive_buffers = [[0u8; BUFFER_SIZE]; MAX_MESSAGES];
        let mut iovs: Vec<_> = receive_buffers
            .iter_mut()
            .map(|buf| [io::IoSliceMut::new(&mut buf[..])])
            .collect();

        let cmsg_buffer = nix::cmsg_space!([i32; MAX_FDS_PER_MSG]);
        let mut headers = MultiHeaders::<UnixAddr>::preallocate(MAX_MESSAGES, Some(cmsg_buffer));

        let results = socket::recvmmsg(
            socket.as_raw_fd(),
            &mut headers,
            iovs.iter_mut(),
            socket::MsgFlags::empty(),
            None,
        )?;

        let mut messages = Vec::new();
        for msg in results {
            let mut data = Vec::new();
            for chunk in msg.iovs() {
                data.extend_from_slice(chunk);
            }

            let mut fds = Vec::new();
            if let Ok(cmsgs) = msg.cmsgs() {
                for cmsg in cmsgs {
                    if let ControlMessageOwned::ScmRights(received_fds) = cmsg {
                        fds.extend(received_fds);
                    }
                }
            }

            messages.push(Self { data, fds });
        }

        Ok(messages)
    }
}

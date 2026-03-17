use crate::trace;
use nix::sys::socket::{self, ControlMessageOwned};
use std::os::fd::AsRawFd;
use std::os::unix::net;

pub(crate) struct SocketRawParsedMessage {
    pub(crate) data: Box<[u8]>,
    pub(crate) fds: Vec<i32>,
}

impl SocketRawParsedMessage {
    pub(crate) fn read_from_socket(stream: &net::UnixStream) -> nix::Result<Self> {
        const BUFFER_SIZE: usize = 8192;
        const MAX_FDS_PER_MSG: usize = 255;

        let mut buffer = [0u8; BUFFER_SIZE];
        let mut iov = [std::io::IoSliceMut::new(&mut buffer)];
        let mut cmsg_buf = nix::cmsg_space!([i32; MAX_FDS_PER_MSG]);

        let msg = socket::recvmsg::<()>(
            stream.as_raw_fd(),
            &mut iov,
            Some(&mut cmsg_buf),
            socket::MsgFlags::empty(),
        )?;

        let bytes_received = msg.bytes;
        if bytes_received == 0 {
            return Ok(Self {
                data: [].into(),
                fds: Vec::new(),
            });
        }

        let mut fds = Vec::new();
        for cmsg in msg.cmsgs().map_err(|_| nix::errno::Errno::ENOBUFS)? {
            if let ControlMessageOwned::ScmRights(received_fds) = cmsg {
                trace! {
                    eprintln!(
                        "[hw] trace: SocketRawParsedMessage::read_from_socket: got {} fds on the control wire",
                        received_fds.len()
                    )
                }
                fds.extend(received_fds);
            }
        }

        Ok(Self {
            data: buffer[..bytes_received].into(),
            fds,
        })
    }
}

use crate::trace;
use rustix::net;
use std::os::fd::OwnedFd;
use std::os::{fd, unix};
use std::{io, mem};

pub(crate) struct SocketRawParsedMessage {
    pub(crate) data: Box<[u8]>,
    pub(crate) fds: Vec<i32>,
}

impl SocketRawParsedMessage {
    pub(crate) fn read_from_socket(stream: &unix::net::UnixStream) -> io::Result<Self> {
        const BUFFER_SIZE: usize = 8192;
        const MAX_FDS_PER_MSG: usize = 255;

        let mut data = Vec::new();
        let mut fds = Vec::new();

        loop {
            let mut buffer = [0u8; BUFFER_SIZE];
            let mut iov = [io::IoSliceMut::new(&mut buffer)];
            let mut cmsg_space = vec![
                mem::MaybeUninit::<u8>::uninit();
                rustix::cmsg_space!(ScmRights(MAX_FDS_PER_MSG))
            ];
            let mut cmsg_buf = net::RecvAncillaryBuffer::new(&mut cmsg_space);

            let msg = net::recvmsg(stream, &mut iov, &mut cmsg_buf, net::RecvFlags::empty())
                .map_err(io::Error::from)?;

            let bytes_received = msg.bytes;
            if bytes_received == 0 {
                break;
            }

            for cmsg in cmsg_buf.drain() {
                if let net::RecvAncillaryMessage::ScmRights(received_fds) = cmsg {
                    trace! {
                        crate::log_debug!(
                            "[hw] trace: SocketRawParsedMessage::read_from_socket: got {} fds on the control wire",
                            received_fds.len()
                        )
                    }
                    fds.extend(received_fds.map(|fd: OwnedFd| {
                        use fd::IntoRawFd;
                        fd.into_raw_fd()
                    }));
                }
            }

            data.extend_from_slice(&buffer[..bytes_received]);

            if bytes_received < BUFFER_SIZE {
                break;
            }
        }

        Ok(Self {
            data: data.into_boxed_slice(),
            fds,
        })
    }
}

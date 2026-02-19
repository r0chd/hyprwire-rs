use crate::message::Message;
use crate::{message, steady_millis, trace};
use nix::sys::socket;
use nix::{errno, poll};
use std::os::fd;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::os::unix::net;
use std::{io, path};

pub enum SocketSource<'a> {
    Path(&'a path::Path),
    Fd(fd::RawFd),
}

pub struct ClientSocket {
    stream: net::UnixStream,
}

impl ClientSocket {
    pub fn open(source: SocketSource) -> Self {
        let stream = match source {
            SocketSource::Path(path) => {
                net::UnixStream::connect(path).expect("Failed to connect to Unix socket")
            }
            SocketSource::Fd(fd) => unsafe { net::UnixStream::from_raw_fd(fd) },
        };

        let client_socket = Self { stream };
        client_socket.send_message(message::Hello::new());

        client_socket
    }

    pub fn send_message<T>(&self, message: T)
    where
        T: Message,
    {
        trace! { log::trace!("[{} @ {:.3}] -> {}", self.stream.as_raw_fd(), steady_millis(), message.parse_data()) };

        let buf = message.get_data();
        let iov = [io::IoSlice::new(buf)];
        let cmsg = [socket::ControlMessage::ScmRights(message.get_fds())];
        loop {
            match socket::sendmsg::<()>(
                self.stream.as_raw_fd(),
                &iov,
                &cmsg,
                socket::MsgFlags::empty(),
                None,
            ) {
                Ok(_) => break,
                Err(errno::Errno::EAGAIN) => {
                    let mut pfd = [poll::PollFd::new(
                        self.stream.as_fd(),
                        poll::PollFlags::POLLOUT | poll::PollFlags::POLLWRBAND,
                    )];
                    _ = poll::poll(&mut pfd, poll::PollTimeout::NONE);
                    continue;
                }
                Err(_) => {
                    break;
                }
            }
        }
    }
}

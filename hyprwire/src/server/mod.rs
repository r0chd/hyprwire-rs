/// Server-side APIs for hosting Hyprwire protocols and dispatching client
/// requests.
pub mod server_client;
pub(crate) mod server_object;
mod server_socket;

pub use server_client::ServerClient;
pub use server_socket::ServerSocket as Server;

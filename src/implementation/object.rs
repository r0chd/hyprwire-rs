use crate::client;

pub trait Object {
    // listen

    fn client_sock(&self) -> Option<&client::ClientSocket<'_>> {
        None
    }

    // fn server_sock(&self) -> Option<>,

    // set_data
    //
    // get_data

    fn error<'a>(&self, error_id: u32, error_msg: &'a str);

    // fn get_client(&self) -> ServerC
}

use hyprwire::implementation as hw;

pub mod client {
    use super::hw;
    use hyprwire::{Dispatch, DispatchData, Proxy};
    use std::cell::RefCell;
    use std::ffi::{c_char, c_void, CStr};
    use std::rc::Rc;

    pub enum MyManagerV1Event<'a> {
        SendMessage { message: &'a CStr },
        RecvMessageArrayUint { message: &'a [u32] },
    }

    pub struct MyManagerV1Object {
        object: hw::types::Object,
    }

    impl Proxy for MyManagerV1Object {
        type Event<'a> = MyManagerV1Event<'a>;
    }

    unsafe extern "C" fn my_manager_v1_send_message<D: Dispatch<MyManagerV1Object>>(
        data: *mut c_void,
        message: *const c_char,
    ) {
        let dispatch = unsafe { &*(data as *const DispatchData<D>) };
        let state = unsafe { &mut *dispatch.state };
        let proxy = MyManagerV1Object {
            object: hw::types::Object::from_raw(dispatch.object.inner().clone()),
        };
        let message = unsafe { CStr::from_ptr(message) };
        state.event(&proxy, MyManagerV1Event::SendMessage { message });
    }

    unsafe extern "C" fn my_manager_v1_recv_message_array_uint<D: Dispatch<MyManagerV1Object>>(
        data: *mut c_void,
        message: *const u32,
        message_len: u32,
    ) {
        let dispatch = unsafe { &*(data as *const DispatchData<D>) };
        let state = unsafe { &mut *dispatch.state };
        let proxy = MyManagerV1Object {
            object: hw::types::Object::from_raw(dispatch.object.inner().clone()),
        };
        let message = unsafe { std::slice::from_raw_parts(message, message_len as usize) };
        state.event(&proxy, MyManagerV1Event::RecvMessageArrayUint { message });
    }

    impl MyManagerV1Object {
        pub fn new<D: Dispatch<Self>>(object: hw::types::Object, state: &mut D) -> Self {
            let dispatch_data = Box::into_raw(Box::new(DispatchData {
                state: state as *mut D,
                object: hw::types::Object::from_raw(object.inner().clone()),
            }));

            {
                let mut obj = object.inner().borrow_mut();
                obj.set_data(dispatch_data as *mut c_void);
                obj.listen(0, my_manager_v1_send_message::<D> as *mut c_void);
                obj.listen(1, my_manager_v1_recv_message_array_uint::<D> as *mut c_void);
            }

            Self { object }
        }

        pub fn send_send_message(&self, message: &str) {
            self.object
                .inner()
                .borrow_mut()
                .call(0, &[hw::types::CallArg::Varchar(message.as_bytes())]);
        }

        pub fn send_send_message_fd(&self, fd: i32) {
            self.object
                .inner()
                .borrow_mut()
                .call(1, &[hw::types::CallArg::Fd(fd)]);
        }

        pub fn send_send_message_array_fd(&self, fds: &[i32]) {
            self.object
                .inner()
                .borrow_mut()
                .call(2, &[hw::types::CallArg::FdArray(fds)]);
        }

        pub fn send_send_message_array(&self, msgs: &[&str]) {
            let bytes: Vec<&[u8]> = msgs.iter().map(|s| s.as_bytes()).collect();
            self.object
                .inner()
                .borrow_mut()
                .call(3, &[hw::types::CallArg::VarcharArray(&bytes)]);
        }

        pub fn send_send_message_array_uint(&self, vals: &[u32]) {
            self.object
                .inner()
                .borrow_mut()
                .call(4, &[hw::types::CallArg::UintArray(vals)]);
        }

        pub fn send_make_object(&self) -> Option<hw::types::Object> {
            let seq = self.object.inner().borrow_mut().call(5, &[]);
            let obj = self
                .object
                .inner()
                .borrow()
                .client_sock()
                .and_then(|sock| sock.borrow().object_for_seq(seq))
                .map(|obj| obj as Rc<RefCell<dyn hw::object::Object>>)?;
            Some(hw::types::Object::from_raw(obj))
        }
    }

    pub enum MyObjectV1Event<'a> {
        SendMessage { message: &'a CStr },
    }

    pub struct MyObjectV1Object {
        object: hw::types::Object,
    }

    impl Proxy for MyObjectV1Object {
        type Event<'a> = MyObjectV1Event<'a>;
    }

    unsafe extern "C" fn my_object_v1_send_message<D: Dispatch<MyObjectV1Object>>(
        data: *mut c_void,
        message: *const c_char,
    ) {
        let dispatch = unsafe { &*(data as *const DispatchData<D>) };
        let state = unsafe { &mut *dispatch.state };
        let proxy = MyObjectV1Object {
            object: hw::types::Object::from_raw(dispatch.object.inner().clone()),
        };
        let message = unsafe { CStr::from_ptr(message) };
        state.event(&proxy, MyObjectV1Event::SendMessage { message });
    }

    impl MyObjectV1Object {
        pub fn new<D: Dispatch<Self>>(object: hw::types::Object, state: &mut D) -> Self {
            let dispatch_data = Box::into_raw(Box::new(DispatchData {
                state: state as *mut D,
                object: hw::types::Object::from_raw(object.inner().clone()),
            }));

            {
                let mut obj = object.inner().borrow_mut();
                obj.set_data(dispatch_data as *mut c_void);
                obj.listen(0, my_object_v1_send_message::<D> as *mut c_void);
            }

            Self { object }
        }

        pub fn send_send_message(&self, message: &str) {
            self.object
                .inner()
                .borrow_mut()
                .call(0, &[hw::types::CallArg::Varchar(message.as_bytes())]);
        }

        pub fn send_send_enum(&self, val: super::spec::MyEnum) {
            self.object
                .inner()
                .borrow_mut()
                .call(1, &[hw::types::CallArg::Uint(val as u32)]);
        }

        pub fn send_destroy(&self) {
            self.object.inner().borrow_mut().call(2, &[]);
        }

        pub fn send_make_object(&self) -> Option<hw::types::Object> {
            let seq = self.object.inner().borrow_mut().call(3, &[]);
            let obj = self
                .object
                .inner()
                .borrow()
                .client_sock()
                .and_then(|sock| sock.borrow().object_for_seq(seq))
                .map(|obj| obj as Rc<RefCell<dyn hw::object::Object>>)?;
            Some(hw::types::Object::from_raw(obj))
        }
    }

    #[derive(Default, Copy, Clone)]
    pub struct TestProtocolV1Impl {
        protocol: super::spec::TestProtocolV1ProtocolSpec,
    }

    impl hw::client::ProtocolImplementations for TestProtocolV1Impl {
        fn protocol(&self) -> &dyn hw::types::ProtocolSpec {
            &self.protocol
        }

        fn implementation(&self) -> &[hw::client::ObjectImplementation<'_>] {
            &[]
        }
    }
}

pub mod spec {
    #[repr(u32)]
    pub enum MyEnum {
        Hello = 0,
        World = 4,
    }

    #[repr(u32)]
    pub enum MyErrorEnum {
        OhNo = 0,
        ErrorImportant = 1,
    }

    pub struct MyManagerV1Spec {
        c2s_methods: &'static [super::hw::types::Method],
        s2c_methods: &'static [super::hw::types::Method],
    }

    static MY_MANAGER_V1: MyManagerV1Spec = MyManagerV1Spec {
        c2s_methods: &[
            super::hw::types::Method {
                idx: 0,
                params: &[super::hw::types::MessageMagic::TypeVarchar as u8],
                returns_type: "",
                since: 0,
            },
            super::hw::types::Method {
                idx: 1,
                params: &[super::hw::types::MessageMagic::TypeFd as u8],
                returns_type: "",
                since: 0,
            },
            super::hw::types::Method {
                idx: 2,
                params: &[
                    super::hw::types::MessageMagic::TypeArray as u8,
                    super::hw::types::MessageMagic::TypeFd as u8,
                ],
                returns_type: "",
                since: 0,
            },
            super::hw::types::Method {
                idx: 3,
                params: &[
                    super::hw::types::MessageMagic::TypeArray as u8,
                    super::hw::types::MessageMagic::TypeVarchar as u8,
                ],
                returns_type: "",
                since: 0,
            },
            super::hw::types::Method {
                idx: 4,
                params: &[
                    super::hw::types::MessageMagic::TypeArray as u8,
                    super::hw::types::MessageMagic::TypeUint as u8,
                ],
                returns_type: "",
                since: 0,
            },
            super::hw::types::Method {
                idx: 5,
                params: &[],
                returns_type: "my_object_v1",
                since: 0,
            },
        ],
        s2c_methods: &[
            super::hw::types::Method {
                idx: 0,
                params: &[super::hw::types::MessageMagic::TypeVarchar as u8],
                returns_type: "",
                since: 0,
            },
            super::hw::types::Method {
                idx: 1,
                params: &[
                    super::hw::types::MessageMagic::TypeArray as u8,
                    super::hw::types::MessageMagic::TypeUint as u8,
                ],
                returns_type: "",
                since: 0,
            },
        ],
    };

    impl super::hw::types::ProtocolObjectSpec for MyManagerV1Spec {
        fn object_name(&self) -> &str {
            "my_manager_v1"
        }

        fn c2s(&self) -> &[super::hw::types::Method] {
            self.c2s_methods
        }

        fn s2c(&self) -> &[super::hw::types::Method] {
            self.s2c_methods
        }
    }

    pub struct MyObjectV1Spec {
        c2s_methods: &'static [super::hw::types::Method],
        s2c_methods: &'static [super::hw::types::Method],
    }

    static MY_OBJECT_V1: MyObjectV1Spec = MyObjectV1Spec {
        c2s_methods: &[
            super::hw::types::Method {
                idx: 0,
                params: &[super::hw::types::MessageMagic::TypeVarchar as u8],
                returns_type: "",
                since: 0,
            },
            super::hw::types::Method {
                idx: 1,
                params: &[super::hw::types::MessageMagic::TypeUint as u8],
                returns_type: "",
                since: 0,
            },
            super::hw::types::Method {
                idx: 2,
                params: &[],
                returns_type: "",
                since: 0,
            },
            super::hw::types::Method {
                idx: 3,
                params: &[],
                returns_type: "my_object_v1",
                since: 0,
            },
        ],
        s2c_methods: &[super::hw::types::Method {
            idx: 0,
            params: &[super::hw::types::MessageMagic::TypeVarchar as u8],
            returns_type: "",
            since: 0,
        }],
    };

    impl super::hw::types::ProtocolObjectSpec for MyObjectV1Spec {
        fn object_name(&self) -> &str {
            "my_object_v1"
        }

        fn c2s(&self) -> &[super::hw::types::Method] {
            self.c2s_methods
        }

        fn s2c(&self) -> &[super::hw::types::Method] {
            self.s2c_methods
        }
    }

    #[derive(Copy, Clone)]
    pub struct TestProtocolV1ProtocolSpec {
        objects: [&'static dyn super::hw::types::ProtocolObjectSpec; 2],
    }

    impl Default for TestProtocolV1ProtocolSpec {
        fn default() -> Self {
            Self {
                objects: [&MY_MANAGER_V1, &MY_OBJECT_V1],
            }
        }
    }

    impl super::hw::types::ProtocolSpec for TestProtocolV1ProtocolSpec {
        fn spec_name(&self) -> &str {
            "test_protocol_v1"
        }

        fn spec_ver(&self) -> u32 {
            1
        }

        fn objects(&self) -> &[&dyn super::hw::types::ProtocolObjectSpec] {
            &self.objects
        }
    }
}

fn main() {}

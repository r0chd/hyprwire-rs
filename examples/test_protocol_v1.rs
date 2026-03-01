use hyprwire::implementation as hw;

pub mod client {
    use std::cell::RefCell;
    use std::ffi::{c_char, c_void, CStr};
    use std::rc::Rc;

    use super::hw;

    pub trait Proxy {
        type Event<'a>;
    }

    pub trait Dispatch<I: Proxy> {
        fn event(&mut self, event: I::Event<'_>);
    }

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
        let state = unsafe { &mut *(data as *mut D) };
        let message = unsafe { CStr::from_ptr(message) };
        state.event(MyManagerV1Event::SendMessage { message });
    }

    unsafe extern "C" fn my_manager_v1_recv_message_array_uint<D: Dispatch<MyManagerV1Object>>(
        data: *mut c_void,
        message: *const u32,
        message_len: u32,
    ) {
        let state = unsafe { &mut *(data as *mut D) };
        let message = unsafe { std::slice::from_raw_parts(message, message_len as usize) };
        state.event(MyManagerV1Event::RecvMessageArrayUint { message });
    }

    impl MyManagerV1Object {
        pub fn new<D: Dispatch<Self>>(object: hw::types::Object, state: &mut D) -> Self {
            {
                let mut obj = object.inner().borrow_mut();
                obj.set_data(state as *mut D as *mut c_void);
                obj.listen(0, my_manager_v1_send_message::<D> as *mut c_void);
                obj.listen(1, my_manager_v1_recv_message_array_uint::<D> as *mut c_void);
            }

            Self { object }
        }

        pub fn send_send_message(&self, message: &[u8]) {
            self.object
                .inner()
                .borrow_mut()
                .call(0, &[hw::types::CallArg::Varchar(message)]);
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

        pub fn send_send_message_array(&self, msgs: &[&[u8]]) {
            self.object
                .inner()
                .borrow_mut()
                .call(3, &[hw::types::CallArg::VarcharArray(msgs)]);
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
        let state = unsafe { &mut *(data as *mut D) };
        let message = unsafe { CStr::from_ptr(message) };
        state.event(MyObjectV1Event::SendMessage { message });
    }

    impl MyObjectV1Object {
        pub fn new<D: Dispatch<Self>>(object: hw::types::Object, state: &mut D) -> Self {
            {
                let mut obj = object.inner().borrow_mut();
                obj.set_data(state as *mut D as *mut c_void);
                obj.listen(0, my_object_v1_send_message::<D> as *mut c_void);
            }

            Self { object }
        }

        pub fn send_send_message(&self, message: &[u8]) {
            self.object
                .inner()
                .borrow_mut()
                .call(0, &[hw::types::CallArg::Varchar(message)]);
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

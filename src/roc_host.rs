use libc::c_void;
use roc_std::RocStr;

pub fn init() {
    let funcs: &[*const extern "C" fn()] = &[
        roc_alloc as _,
        roc_realloc as _,
        roc_dealloc as _,
        roc_panic as _,
    ];
    std::hint::black_box(funcs);
}

#[no_mangle]
pub unsafe extern "C" fn roc_alloc(size: usize, _alignment: u32) -> *mut c_void {
    libc::malloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn roc_realloc(
    c_ptr: *mut c_void,
    new_size: usize,
    _old_size: usize,
    _alignment: u32,
) -> *mut c_void {
    libc::realloc(c_ptr, new_size)
}

#[no_mangle]
pub unsafe extern "C" fn roc_dealloc(c_ptr: *mut c_void, _alignment: u32) {
    libc::free(c_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn roc_panic(msg: *const RocStr, _tag_id: u32) {
    panic!("{}", *msg);
}

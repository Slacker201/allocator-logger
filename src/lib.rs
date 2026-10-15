#![allow(internal_features)]
#![feature(core_intrinsics)]
use std::{alloc::{GlobalAlloc, System}, fs::{File, OpenOptions}, intrinsics::return_address, ptr, sync::{Once, atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering}}};

use chrono::Local;

pub struct CustomAlloc;

static POINTER: AtomicPtr<LoggingData> = AtomicPtr::new(ptr::null_mut());
static FILE: AtomicPtr<File> = AtomicPtr::new(ptr::null_mut());
static COUNTER: AtomicUsize = AtomicUsize::new(0);
static ALLOCATING_FILE: AtomicBool = AtomicBool::new(false);
static REGISTER_EXIT: Once = Once::new();

enum AllocType {
    Allocation,
    Deallocation,
}

#[allow(unused)]
struct LoggingData {
    size: usize,
    call_site: *const (),
    pointer: *mut u8,
    ty: AllocType,
}

#[global_allocator]
static GLOBAL: CustomAlloc = CustomAlloc;


unsafe extern "C" {
    fn atexit(cb: extern "C" fn()) -> i32;
}

extern "C" fn trim_log_on_exit() {
    let count = COUNTER.load(Ordering::Relaxed);
    let bytes_used = count * std::mem::size_of::<LoggingData>();

    let log_pointer = POINTER.load(Ordering::Relaxed);
    if log_pointer.is_null() {
        return;
    }

    let file = unsafe { &*FILE.load(Ordering::Relaxed) };
    let _ = file.set_len(bytes_used as u64);
}


unsafe impl GlobalAlloc for CustomAlloc {
    #[inline(never)]
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        

        let pointer = unsafe { System.alloc(layout) };

        if POINTER.load(std::sync::atomic::Ordering::Relaxed).is_null() {
            REGISTER_EXIT.call_once(|| {
                unsafe { atexit(trim_log_on_exit) };
            });
            alloc_file();
        }
        let write_ptr = POINTER.load(Ordering::Relaxed);

        if !write_ptr.is_null() {
            let size = layout.size();
            let call_site = return_address();

            let logging_data = LoggingData {
                size,
                pointer,
                call_site,
                ty: AllocType::Allocation,
            };

            let offset = COUNTER.fetch_add(1, Ordering::Relaxed);

            let file_pointer = POINTER.load(Ordering::Relaxed);

            unsafe {
                file_pointer.add(offset).write(logging_data);
            }
        }
        pointer
    }

    #[inline(never)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {

        
        let file_pointer = POINTER.load(Ordering::Relaxed);

        if !file_pointer.is_null() {
            let size = layout.size();
            let call_site = return_address();

            let logging_data = LoggingData {
                size,
                pointer: ptr,
                call_site,
                ty: AllocType::Deallocation,
            };


            let offset = COUNTER.fetch_add(1, Ordering::Relaxed);


            unsafe {
                file_pointer.add(offset).write(logging_data);
            }
        }

        unsafe { System.dealloc(ptr, layout) }
    }
}

const MEGABYTE: u64 = 1048576;
const LOGGING_LENGTH: u64 = MEGABYTE * 16;

fn alloc_file() {
    if !ALLOCATING_FILE.fetch_or(true, std::sync::atomic::Ordering::Relaxed) {

        let time = Local::now().format("%Y-%m-%d-%H-%M-%S").to_string();

        let Ok(Some(mut path)) = std::env::current_exe().map(|path| path.parent().map(|c| c.to_path_buf())) else {
            ALLOCATING_FILE.store(false, Ordering::Relaxed);
            return;
        };

        path = path.join("logs");
        path = path.join(format!("allocator_logs_{}.alog", time));

        let file = OpenOptions::new().create(true).read(true).write(true).open(path);

        if let Ok(file) = file {
            let _ = file.set_len(LOGGING_LENGTH);
            let mmap_result = unsafe { memmap2::MmapMut::map_mut(&file) };

            if let Ok(mmap_handle) = mmap_result {
                let boxed_mmap_handler = Box::new(mmap_handle);

                let mmap_pointer = boxed_mmap_handler.as_ptr();

                Box::leak(boxed_mmap_handler);
                let boxed_file = Box::new(file);

                let file_ptr = Box::into_raw(boxed_file);

                FILE.store(file_ptr, Ordering::Relaxed);

                POINTER.store(mmap_pointer as *mut LoggingData, Ordering::Relaxed);

            }
            
            ALLOCATING_FILE.store(false, Ordering::Relaxed);
        } else {
            ALLOCATING_FILE.store(false, Ordering::Relaxed);
        }
    }
}
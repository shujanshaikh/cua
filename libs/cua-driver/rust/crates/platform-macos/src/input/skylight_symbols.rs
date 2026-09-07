//! Read local symbols in the SkyLight image already loaded in this process.
//! Adapted from yabai's misc/macho_dlsym.h (MIT, Åsmund Vikane).
//! No other process is read, modified, or injected into.

use std::ffi::{c_char, c_void, CStr};

#[repr(C)]
struct Header {
    magic: u32,
    cpu: u32,
    subtype: u32,
    filetype: u32,
    commands: u32,
    bytes: u32,
    flags: u32,
    reserved: u32,
}
#[repr(C)]
struct Segment {
    cmd: u32,
    size: u32,
    name: [u8; 16],
    address: u64,
    length: u64,
    offset: u64,
    file_length: u64,
    maxprot: u32,
    initprot: u32,
    sections: u32,
    flags: u32,
}
#[repr(C)]
struct Symtab {
    cmd: u32,
    size: u32,
    offset: u32,
    count: u32,
    strings: u32,
    string_bytes: u32,
}
#[repr(C)]
struct Symbol {
    string: u32,
    kind: u8,
    section: u8,
    desc: u16,
    value: u64,
}

extern "C" {
    fn _dyld_image_count() -> u32;
    fn _dyld_get_image_name(index: u32) -> *const c_char;
    fn _dyld_get_image_header(index: u32) -> *const Header;
    fn _dyld_get_image_vmaddr_slide(index: u32) -> isize;
}

/// Only accepts the OS-owned SkyLight image. Missing/stripped symbols fail closed.
pub(super) fn find_local(name: &CStr) -> Option<*mut c_void> {
    // Ensure SkyLight is loaded through the existing integration first.
    super::skylight::find_sym(b"SLSMainConnectionID\0")?;
    unsafe {
        for index in 0.._dyld_image_count() {
            let path = _dyld_get_image_name(index);
            if path.is_null()
                || CStr::from_ptr(path).to_bytes()
                    != b"/System/Library/PrivateFrameworks/SkyLight.framework/Versions/A/SkyLight"
            {
                continue;
            }
            let header = _dyld_get_image_header(index).as_ref()?;
            if header.magic != 0xfeedfacf {
                return None;
            }
            let base = (header as *const Header).cast::<u8>();
            let end = std::mem::size_of::<Header>().checked_add(header.bytes as usize)?;
            let mut offset = std::mem::size_of::<Header>();
            let mut linkedit = None;
            let mut symtab = None;
            for _ in 0..header.commands {
                if offset.checked_add(8)? > end {
                    return None;
                }
                let cmd = base.add(offset).cast::<u32>();
                let size = *cmd.add(1) as usize;
                if size < 8 || offset.checked_add(size)? > end {
                    return None;
                }
                if *cmd == 0x19 && size >= std::mem::size_of::<Segment>() {
                    let segment = &*cmd.cast::<Segment>();
                    if segment.name == *b"__LINKEDIT\0\0\0\0\0\0" {
                        linkedit = Some(segment);
                    }
                } else if *cmd == 2 && size >= std::mem::size_of::<Symtab>() {
                    symtab = Some(&*cmd.cast::<Symtab>());
                }
                offset += size;
            }
            let segment = linkedit?;
            let table = symtab?;
            let slide = _dyld_get_image_vmaddr_slide(index);
            let link_base = (segment.address as usize)
                .wrapping_add_signed(slide)
                .checked_sub(segment.offset as usize)?;
            let within = |offset: u64, length: u64| {
                offset >= segment.offset
                    && offset.checked_add(length).is_some_and(|end| {
                        end <= segment.offset.saturating_add(segment.file_length)
                    })
            };
            if !within(table.offset as u64, table.count as u64 * 16)
                || !within(table.strings as u64, table.string_bytes as u64)
            {
                return None;
            }
            let symbols = (link_base.checked_add(table.offset as usize)?) as *const Symbol;
            let strings = (link_base.checked_add(table.strings as usize)?) as *const u8;
            for i in 0..table.count as usize {
                let symbol = &*symbols.add(i);
                let start = symbol.string as usize;
                let len = name.to_bytes_with_nul().len();
                if symbol.kind & 0xe0 != 0
                    || symbol.kind & 0x0e != 0x0e
                    || symbol.value == 0
                    || start.checked_add(len)? > table.string_bytes as usize
                {
                    continue;
                }
                if std::slice::from_raw_parts(strings.add(start), len) == name.to_bytes_with_nul() {
                    return Some((symbol.value as usize).wrapping_add_signed(slide) as *mut c_void);
                }
            }
            return None;
        }
    }
    None
}

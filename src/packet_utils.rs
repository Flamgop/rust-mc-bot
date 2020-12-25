use std::intrinsics::copy_nonoverlapping;
use std::mem;
use std::convert::TryInto;

pub struct Buf {
    pub buffer: Vec<u8>,
    write_index: u32,
    read_index: u32,
    write_mark: u32,
    read_mark: u32,
}

impl Buf {
    pub fn new() -> Buf {
        Buf { buffer: Vec::new(), write_index: 0, read_index: 0, write_mark: 0, read_mark: 0 }
    }
    pub fn with_length(length: u32) -> Buf {
        Buf { buffer: vec![0u8; length as usize], write_index: 0, read_index: 0, write_mark: 0, read_mark: 0 }
    }
    pub fn with_capacity(capacity: u32) -> Buf {
        Buf { buffer: Vec::with_capacity(capacity as usize), write_index: 0, read_index: 0, write_mark: 0, read_mark: 0 }
    }
    pub fn from_vec(vec : Vec<u8>) -> Buf {
        Buf { buffer: vec, write_index: 0, read_index: 0, write_mark: 0, read_mark: 0 }
    }

    pub fn is_nonoverlapping<T>(src: *const T, dst: *const T, count: usize) -> bool {
        let src_usize = src as usize;
        let dst_usize = dst as usize;
        let size = mem::size_of::<T>().checked_mul(count).unwrap();
        let diff = if src_usize > dst_usize { src_usize - dst_usize } else { dst_usize - src_usize };
        // If the absolute distance between the ptrs is at least as big as the size of the buffer,
        // they do not overlap.
        diff >= size
    }

    pub fn write_bytes(&mut self, other: &[u8]) {
        unsafe { self.mem_cpy(other.as_ptr(), 0, other.len()); }
    }

    pub unsafe fn mem_cpy(&mut self, other: *const u8, start: u32, len: usize) {
        let dst = &mut self.buffer;
        let extra = (len as u32 + self.write_index) as i32 - dst.len() as i32;
        if extra > 0 { dst.reserve(extra as usize); }
        let dst_ptr = dst.as_mut_ptr().offset(self.write_index as isize);
        let src_ptr = other.offset(start as isize);
        if Self::is_nonoverlapping(src_ptr, dst_ptr, len as usize) {
            copy_nonoverlapping(src_ptr, dst_ptr, len as usize);
        } else {
            panic!("copy is overlapping")
        }

        if extra > 0 { dst.set_len(dst.len() + extra as usize); }
        self.write_index += len as u32;
    }

    pub fn append(&mut self, other: Buf) {
        self.write_bytes(other.buffer.as_slice());
    }

    pub fn ensure_writable(&mut self, num: u32) {
        if self.buffer.len() as isize - (self.write_index as isize) < num as isize {
            self.buffer.reserve(self.write_index as usize + num as usize - self.buffer.len());
            unsafe { self.buffer.set_len(self.buffer.len() + num as usize); }
        }
    }

    pub fn write_u8(&mut self, num: u8) {
        self.ensure_writable(1);
        self.buffer[self.write_index as usize] = num;
        self.write_index += 1;
    }

    pub fn write_bool(&mut self, b: bool) {
        self.write_u8(if b { 1 } else { 0 });
    }

    pub fn write_u16(&mut self, num: u16) {
        self.write_bytes(&num.to_be_bytes());
    }

    pub fn write_u32(&mut self, num: u32) {
        self.write_bytes(&num.to_be_bytes());
    }

    pub fn write_u64(&mut self, num: u64) {
        self.write_bytes(&num.to_be_bytes());
    }

    // This is for uuids too
    pub fn write_u128(&mut self, num: u128) {
        self.write_bytes(&num.to_be_bytes());
    }

    pub fn write_f32(&mut self, num: f32) {
        self.write_u32(num.to_bits());
    }

    pub fn write_f64(&mut self, num: f64) {
        self.write_u64(num.to_bits());
    }

    pub fn write_var_u32(&mut self, num: u32) {
        let mut number = num;
        loop {
            let mut temp = number as u8 & 0b01111111;
            number >>= 7;
            if number != 0 {
                temp |= 0b10000000;
            }
            self.write_u8(temp);
            if number == 0 {
                break;
            }
        }
    }

    pub fn write_var_u64(&mut self, num: u64) {
        let mut number = num;
        loop {
            let mut temp = number as u8 & 0b01111111;
            number >>= 7;
            if number != 0 {
                temp |= 0b10000000;
            }
            self.write_u8(temp);
            if number == 0 {
                break;
            }
        }
    }

    pub fn write_sized_str(&mut self, string: &str) {
        let bytes = string.as_bytes();
        self.write_var_u32(bytes.len() as u32);
        self.write_bytes(bytes);
    }

    pub fn write_short_sized_str(&mut self, string: &str) {
        let bytes = string.as_bytes();
        self.write_u16(bytes.len() as u16);
        self.write_bytes(bytes);
    }

    pub fn write_var_u32_slice(&mut self, slice: &[u32]) {
        self.write_var_u32(slice.len() as u32);
        for i in slice {
            self.write_var_u32(*i);
        }
    }

    pub fn write_str_slice(&mut self, slice: &[&str]) {
        self.write_var_u32(slice.len() as u32);
        for i in slice {
            self.write_sized_str(i);
        }
    }

    pub fn write_block_position(&mut self, x: i32, y: i32, z: i32) {
        self.write_u64((x as u64 & 0x3FFFFFF) << 38 | (z as u64 & 0x3FFFFFF) << 12 | y as u64 & 0xFFF);
    }
    
    pub fn write_packet_id(&mut self, num: u32) {
        self.write_var_u32(num);
    }

    pub fn read_byte(&mut self) -> u8 {
        let byte: u8 = self.buffer[self.read_index as usize];
        self.read_index += 1;
        byte
    }

    pub fn read_bool(&mut self) -> bool {
        if self.read_byte() == 1 {
            true
        } else {
            false
        }
    }

    pub fn read_u16(&mut self) -> u16 {
        let index = self.read_index as usize;
        let num: [u8;2] = self.buffer[index..index+2].try_into().unwrap();
        self.read_index += 2;
        unsafe { u16::from_be(mem::transmute_copy(&num)) }
    }

    pub fn read_u32(&mut self) -> u32 {
        let index = self.read_index as usize;
        let num: [u8;4] = self.buffer[index..index+4].try_into().unwrap();
        self.read_index += 4;
        unsafe { u32::from_be(mem::transmute_copy(&num)) }
    }

    pub fn read_u64(&mut self) -> u64 {
        let index = self.read_index as usize;
        let num: [u8;8] = self.buffer[index..index+8].try_into().unwrap();
        self.read_index += 8;
        unsafe { u64::from_be(mem::transmute_copy(&num)) }
    }

    pub fn read_u128(&mut self) -> u128 {
        let index = self.read_index as usize;
        let num: [u8;16] = self.buffer[index..index+16].try_into().unwrap();
        self.read_index += 16;
        unsafe { u128::from_be(mem::transmute_copy(&num)) }
    }

    pub fn read_f32(&mut self) -> f32 {
        f32::from_bits(self.read_u32())
    }

    pub fn read_f64(&mut self) -> f64 {
        f64::from_bits(self.read_u64())
    }

    pub fn read_bytes(&mut self, length : u32) -> &[u8] {
        let bytes: &[u8] = &self.buffer[self.read_index as usize..(self.read_index+length) as usize];
        self.read_index += length;
        bytes
    }

    pub fn read_sized_string(&mut self) -> &str {
        let length = self.read_var_u32();
        let bytes = self.read_bytes(length);
        std::str::from_utf8(bytes).expect("Error occurred while parsing string")
    }

    pub fn read_short_sized_string(&mut self) -> &str {
        let length = self.read_u16();
        let bytes = self.read_bytes(length as u32);
        std::str::from_utf8(bytes).expect("Error occurred while parsing string")
    }

    pub fn read_var_u32_slice(&mut self) -> Vec<u32> {
        let length = self.read_var_u32();
        let mut nums: Vec<u32> = Vec::with_capacity(length as usize);
        for _ in 0..length {
            nums.push(self.read_var_u32());
        }
        nums
    }

    pub fn read_var_u32(&mut self) -> u32 {
        let mut num_read = 0u32;
        let mut result = 0u32;
        let mut read;
        loop {
            read = self.read_byte() as u32;
            result |= (read & 0b01111111).overflowing_shl(7 * num_read).0;
            num_read += 1;
            if num_read > 5 {
                panic!("VarInt is too big")
            }
            if read & 0b10000000 == 0 {
                break;
            }
        }
        result
    }

    pub fn read_var_u64(&mut self) -> u64 {
        let mut num_read = 0u64;
        let mut result = 0u64;
        let mut read;
        loop {
            read = self.read_byte() as u64;
            result |= (read & 0b01111111) << (7 * num_read);
            num_read += 1;
            if num_read > 10 {
                panic!("VarLong is too big")
            }
            if read & 0b10000000 == 0 {
                break;
            }
        }
        result
    }

    pub fn read_block_position(&mut self) -> (i32, u8, i32) {
        let value = self.read_u64();
        let x = (value >> 38) as i32;
        let z = (value >> 12) as i32;
        let y = (value & 0xFFF) as u8;
        (x,y,z)
    }

    pub fn mark_reader(&mut self) {
        self.read_mark = self.read_index;
    }

    pub fn reset_reader(&mut self) {
        self.read_index = self.read_mark;
    }

    pub fn mark_writer(&mut self) {
        self.write_mark = self.write_index;
    }

    pub fn reset_writer(&mut self) {
        self.write_index = self.write_mark;
    }

    pub fn set_reader_index(&mut self, index : u32) {
        self.read_index = index;
    }

    pub fn set_writer_index(&mut self, index : u32) {
        self.write_index = index;
    }

    pub fn get_reader_index(&self) -> u32 {
        self.read_index
    }

    pub fn get_writer_index(&self) -> u32 {
        self.write_index
    }

    pub fn get_var_u32_size(num : u32) -> u32 {
        if num & 0xFFFFFF80 == 0 {
            1
        } else if num & 0xFFFFC000 == 0 {
            2
        } else if num & 0xFFE00000 == 0 {
            3
        } else if num & 0xF0000000 == 0 {
            4
        } else {
            5
        }
    }
}
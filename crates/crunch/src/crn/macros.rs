macro_rules! CRND_HUFF_DECODE {
    ($codec: expr, $model: expr, $symbol: expr) => {
        $symbol = match $codec.decode($model) {
            Ok(s) => s,
            Err(_) => return false,
        }
    };
}

macro_rules! WRITE_TO_INT_BUFFER {
    ($buf: expr, $index: expr, $val: expr) => {
        let t_index = ($index * 4) as usize;
        #[cfg(target_endian = "little")]
        let tiles = $val.to_le_bytes();
        #[cfg(target_endian = "big")]
        let tiles = $val.to_be_bytes();
        $buf[t_index] = tiles[0];
        $buf[t_index + 1] = tiles[1];
        $buf[t_index + 2] = tiles[2];
        $buf[t_index + 3] = tiles[3]
    };
}

macro_rules! WRITE_OR_U8_INTO_U16_BUFFER {
    ($buf: expr, $index: expr, $val: expr) => {
        let t_index = ($index >> 1) as usize;
        let t_val = $buf[t_index].to_be_bytes();
        if $index & 1 != 1 {
            $buf[t_index] = ((t_val[0] as u16) << 8) | ((t_val[1] as u16) | $val as u16);
        } else {
            $buf[t_index] = (((t_val[0] as u16) | ($val as u16)) << 8) | (t_val[1] as u16);
        }
    };
}

pub(crate) use {CRND_HUFF_DECODE, WRITE_OR_U8_INTO_U16_BUFFER, WRITE_TO_INT_BUFFER};

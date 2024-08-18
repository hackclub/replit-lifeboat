use anyhow::{format_err, Result};
use crc32fast::Hasher;
use crosis::goval::{self, OtPacket};
use ropey::Rope;

const STEP_SIZE: i64 = 60 * 60;
const STEP_SIZE_HALF: i64 = STEP_SIZE / 2;

pub fn normalize_ts(ts: i64, start_ts: i64) -> i64 {
    if ts < start_ts {
        return start_ts;
    }

    let adj_offset = start_ts % STEP_SIZE;

    STEP_SIZE * ((ts + STEP_SIZE_HALF - adj_offset) / STEP_SIZE) + adj_offset
}

pub fn do_ot(contents: &mut Rope, ot: &OtPacket) -> Result<()> {
    let mut cursor: usize = 0;

    for op in &ot.op {
        let Some(component) = &op.op_component else {
            return Err(format_err!("Ot packet without components"));
        };

        match component {
            goval::ot_op_component::OpComponent::Skip(_skip) => {
                let skip: usize = (*_skip).try_into()?;
                if skip + cursor > contents.len_chars() {
                    return Err(format_err!("Invalid skip past bounds"));
                }

                cursor += skip;
            }
            goval::ot_op_component::OpComponent::Delete(_delete) => {
                let delete: usize = (*_delete).try_into()?;
                if delete + cursor > contents.len_chars() {
                    return Err(format_err!("Invalid delete past bounds"));
                }

                contents.remove(cursor..(cursor + delete));
            }
            goval::ot_op_component::OpComponent::Insert(insert) => {
                contents.insert(cursor, insert);
            }
        }
    }

    let mut crc32_hasher = Hasher::new();

    for chunk in contents.chunks() {
        let bytes = chunk.as_bytes();

        crc32_hasher.update(bytes);
    }

    let crc32 = crc32_hasher.finalize();

    if crc32 == ot.crc32 {
        Ok(())
    } else {
        Err(format_err!("Expected crc32 to be {} got {crc32}", ot.crc32))
    }
}

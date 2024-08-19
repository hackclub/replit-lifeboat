use anyhow::{format_err, Result};
use async_zip::{base::write::ZipFileWriter, ZipEntryBuilder};
use crc32fast::Hasher;
use crosis::goval::{self, OtPacket};
use ropey::Rope;
use tokio::fs;

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

                cursor += insert.len();
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

pub async fn recursively_flatten_dir(dir: String) -> Result<Vec<String>> {
    let mut fres = fs::read_dir(&dir).await?;

    let mut path = dir;

    let mut files_list = vec![];
    let mut to_check_dirs = vec![];

    loop {
        while let Some(file) = fres.next_entry().await? {
            let fname = if let Ok(string) = file.file_name().into_string() {
                string
            } else {
                return Err(format_err!("Invalid file path"));
            };

            let fpath: String = if path.is_empty() {
                fname
            } else {
                format!("{}/{}", path.clone(), &fname)
            };

            let ftype = file.file_type().await?;

            if ftype.is_dir() {
                to_check_dirs.push(fpath)
            } else if ftype.is_file() {
                files_list.push(fpath);
            } else {
                return Err(format_err!("Invalid file in dir"));
            }
        }

        if let Some(npath) = to_check_dirs.pop() {
            path = npath;

            fres = fs::read_dir(&path).await?;
        } else {
            break;
        }
    }

    Ok(files_list)
}

pub async fn make_zip(dir: String, zip_path: String) -> Result<()> {
    let mut file = fs::File::create(zip_path).await?;
    let mut writer = ZipFileWriter::with_tokio(&mut file);

    let files = recursively_flatten_dir(dir.clone()).await?;

    let file_prefix = dir + "/";

    for file in files {
        let data = fs::read(&file).await?;
        let builder = ZipEntryBuilder::new(
            file.strip_prefix(&file_prefix).unwrap_or(&file).into(),
            async_zip::Compression::Deflate,
        );

        writer.write_entry_whole(builder, &data).await?;
    }
    writer.close().await?;

    Ok(())
}

//! File extractors. Each returns the item's kind, a size label, and locator-tagged
//! units for the chunker. Adding a format means adding one arm to `extract`.

use crate::chunk::Unit;
use anyhow::{Context, Result};
use std::path::Path;

pub struct Extracted {
    pub kind: &'static str,
    pub size_label: String,
    pub size_num: i64,
    pub units: Vec<Unit>,
}

const CODE_EXT: &[&str] = &[
    "py", "rs", "js", "ts", "tsx", "jsx", "go", "java", "c", "cc", "cpp", "h", "hpp", "cs",
    "rb", "php", "swift", "kt", "sh", "bash", "ps1", "sql", "toml", "yaml", "yml", "json",
    "html", "css", "scss", "lua", "r", "jl", "ex", "exs", "zig", "dart",
];
const TEXT_EXT: &[&str] = &["txt", "md", "markdown", "rst", "log", "csv", "tsv"];
const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];

pub fn supported(path: &Path) -> bool {
    let ext = ext_of(path);
    matches!(ext.as_str(), "pdf" | "docx" | "xlsx" | "xls")
        || CODE_EXT.contains(&ext.as_str())
        || TEXT_EXT.contains(&ext.as_str())
        || IMAGE_EXT.contains(&ext.as_str())
}

/// The kind an extractor would assign, from the extension alone.
pub fn kind_of(path: &Path) -> &'static str {
    let ext = ext_of(path);
    match ext.as_str() {
        "pdf" => "pdf",
        "docx" => "document",
        "xlsx" | "xls" => "spreadsheet",
        e if CODE_EXT.contains(&e) => "code",
        e if TEXT_EXT.contains(&e) => "text",
        e if IMAGE_EXT.contains(&e) => "image",
        _ => "other",
    }
}

pub fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The largest file of each kind Recall will read, whatever the size setting
/// says: past this a reader can run out of memory, which ends the whole app.
pub fn hard_cap(path: &Path) -> u64 {
    const MB: u64 = 1024 * 1024;
    match ext_of(path).as_str() {
        "pdf" => 256 * MB,
        "docx" | "xlsx" => 128 * MB,
        "xls" => 32 * MB,
        e if IMAGE_EXT.contains(&e) => u64::MAX, // only the name is read
        _ => 32 * MB,
    }
}

/// A Word or Excel file is a zip. Refuse one that would unpack to far more
/// than any real document (a zip bomb) before a reader inflates it.
fn zip_ok(path: &Path) -> Result<()> {
    const MAX_ENTRIES: usize = 20_000;
    const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?).context("open zip")?;
    anyhow::ensure!(zip.len() <= MAX_ENTRIES, "too many parts ({})", zip.len());
    let mut total = 0u64;
    for i in 0..zip.len() {
        total = total.saturating_add(zip.by_index_raw(i)?.size());
        anyhow::ensure!(total <= MAX_TOTAL, "unpacks to more than 1 GB");
    }
    Ok(())
}

pub fn extract(path: &Path) -> Result<Extracted> {
    let ext = ext_of(path);
    match ext.as_str() {
        "pdf" => pdf(path),
        "docx" => docx(path),
        "xlsx" | "xls" => spreadsheet(path),
        e if CODE_EXT.contains(&e) => code(path),
        e if TEXT_EXT.contains(&e) => text(path),
        e if IMAGE_EXT.contains(&e) => image(path),
        _ => anyhow::bail!("unsupported: {ext}"),
    }
}

fn pdf(path: &Path) -> Result<Extracted> {
    let bytes = std::fs::read(path)?;
    let pages = pdf_extract::extract_text_from_mem_by_pages(&bytes).context("extract pdf text")?;
    let mut units = Vec::with_capacity(pages.len());
    for (i, t) in pages.iter().enumerate() {
        let n = i + 1;
        if !t.trim().is_empty() {
            units.push(Unit { locator: format!("p.{n}"), text: t.clone(), anchor: Some(n.to_string()) });
        }
    }
    Ok(Extracted {
        kind: "pdf",
        size_label: format!("{} pages", pages.len()),
        size_num: pages.len() as i64,
        units,
    })
}

fn docx(path: &Path) -> Result<Extracted> {
    zip_ok(path)?;
    let bytes = std::fs::read(path)?;
    let doc = docx_rs::read_docx(&bytes).context("read docx")?;
    let mut paras = Vec::new();
    for child in doc.document.children {
        if let docx_rs::DocumentChild::Paragraph(p) = child {
            let t = p.raw_text();
            if !t.trim().is_empty() {
                paras.push(t);
            }
        }
    }
    let words: usize = paras.iter().map(|p| p.split_whitespace().count()).sum();
    // group paragraphs into sections of ~10 so the locator means something
    let units = paras
        .chunks(10)
        .enumerate()
        .map(|(i, grp)| Unit {
            locator: format!("§{}", i + 1),
            text: grp.join("\n\n"),
            anchor: None,
        })
        .collect();
    Ok(Extracted { kind: "document", size_label: format!("{words} words"), size_num: words as i64, units })
}

/// Cells read from one workbook at most; a real sheet has far fewer.
const MAX_CELLS: usize = 2_000_000;

fn spreadsheet(path: &Path) -> Result<Extracted> {
    if ext_of(path) == "xlsx" {
        zip_ok(path)?;
        return xlsx(path);
    }
    use calamine::{open_workbook_auto, Reader};
    let mut wb = open_workbook_auto(path).context("open workbook")?;
    let mut units = Vec::new();
    let mut rows_total = 0usize;
    let names = wb.sheet_names().to_vec();
    for name in &names {
        if let Ok(range) = wb.worksheet_range(name) {
            let mut lines = Vec::new();
            for row in range.rows() {
                rows_total += 1;
                let cells: Vec<String> = row.iter().map(|c| c.to_string()).collect();
                let line = cells.join(" | ");
                if !line.trim_matches(|c| c == '|' || c == ' ').is_empty() {
                    lines.push(line);
                }
            }
            if !lines.is_empty() {
                units.push(Unit { locator: name.clone(), text: lines.join("\n"), anchor: Some(name.clone()) });
            }
        }
    }
    Ok(Extracted {
        kind: "spreadsheet",
        size_label: format!("{rows_total} rows"),
        size_num: rows_total as i64,
        units,
    })
}

/// Reads an .xlsx cell by cell. Loading a whole sheet sizes a grid from its
/// first and last cells, so two far-apart cells could ask for billions.
fn xlsx(path: &Path) -> Result<Extracted> {
    use calamine::{Reader, Xlsx};
    let mut wb: Xlsx<_> = calamine::open_workbook(path).context("open workbook")?;
    let (mut units, mut rows_total, mut cells) = (Vec::new(), 0usize, 0usize);
    let names = wb.sheet_names().to_vec();
    for name in &names {
        let Ok(mut reader) = wb.worksheet_cells_reader(name) else { continue };
        let (mut lines, mut row, mut current): (Vec<String>, Option<u32>, Vec<String>) = (Vec::new(), None, Vec::new());
        let flush = |cur: &mut Vec<String>, lines: &mut Vec<String>| {
            if !cur.is_empty() {
                lines.push(cur.join(" | "));
                cur.clear();
            }
        };
        while let Ok(Some(cell)) = reader.next_cell() {
            cells += 1;
            anyhow::ensure!(cells <= MAX_CELLS, "more than {MAX_CELLS} cells");
            let (r, _) = cell.get_position();
            if row != Some(r) {
                flush(&mut current, &mut lines);
                row = Some(r);
                rows_total += 1;
            }
            let v = calamine::Data::from(cell.get_value().clone()).to_string();
            if !v.trim().is_empty() {
                current.push(v);
            }
        }
        flush(&mut current, &mut lines);
        if !lines.is_empty() {
            units.push(Unit { locator: name.clone(), text: lines.join("\n"), anchor: Some(name.clone()) });
        }
    }
    Ok(Extracted { kind: "spreadsheet", size_label: format!("{rows_total} rows"), size_num: rows_total as i64, units })
}

fn code(path: &Path) -> Result<Extracted> {
    let src = read_text(path)?;
    let lines: Vec<&str> = src.lines().collect();
    // windows of 60 lines, locator is the first line number
    let units = lines
        .chunks(60)
        .enumerate()
        .map(|(i, grp)| {
            let start = i * 60 + 1;
            Unit { locator: format!("L{start}"), text: grp.join("\n"), anchor: Some(start.to_string()) }
        })
        .collect();
    Ok(Extracted {
        kind: "code",
        size_label: format!("{} lines", lines.len()),
        size_num: lines.len() as i64,
        units,
    })
}

fn text(path: &Path) -> Result<Extracted> {
    let src = read_text(path)?;
    let words = src.split_whitespace().count();
    let paras: Vec<&str> = src.split("\n\n").collect();
    let units = paras
        .chunks(8)
        .enumerate()
        .map(|(i, grp)| Unit { locator: format!("§{}", i + 1), text: grp.join("\n\n"), anchor: None })
        .collect();
    Ok(Extracted { kind: "text", size_label: format!("{words} words"), size_num: words as i64, units })
}

fn image(path: &Path) -> Result<Extracted> {
    let meta = std::fs::metadata(path)?;
    let size = human_bytes(meta.len());
    // the only searchable text in a picture is its name
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().replace(['_', '-'], " ");
    let units = vec![Unit { locator: "name".into(), text: stem.clone(), anchor: None }];
    Ok(Extracted { kind: "image", size_label: size, size_num: meta.len() as i64, units })
}

fn read_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn human_bytes(n: u64) -> String {
    const K: f64 = 1024.0;
    let f = n as f64;
    if f < K {
        format!("{n} B")
    } else if f < K * K {
        format!("{:.0} KB", f / K)
    } else if f < K * K * K {
        format!("{:.1} MB", f / K / K)
    } else {
        format!("{:.1} GB", f / K / K / K)
    }
}

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

fn spreadsheet(path: &Path) -> Result<Extracted> {
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
    // Without OCR the only searchable text is the filename. OCR plugs in here
    // (feature "ocr"): run tesseract over the image and push a Unit with locator "OCR".
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().replace(['_', '-'], " ");
    #[allow(unused_mut)]
    let mut units = vec![Unit { locator: "name".into(), text: stem.clone(), anchor: None }];
    #[cfg(feature = "ocr")]
    {
        if let Ok(t) = crate::ocr::run(path) {
            if !t.trim().is_empty() {
                units.push(Unit { locator: "OCR".into(), text: t, anchor: None });
            }
        }
    }
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

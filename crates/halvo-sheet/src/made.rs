//! Workbooks that tests make (r4 U3.22c): the tests of this crate and of the runner write them
//! into a scratch folder. No call of the crate's own reads or writes through this module.

use std::io::{Cursor, Write};

/// The bytes of a workbook whose worksheets are named `names`, in that order, each with no cell:
/// the content types, the package's relationships, the workbook part with one element `sheet`
/// a name, its relationships and one empty worksheet part a name.
pub fn workbook(names: &[&str]) -> Vec<u8> {
    let mut types = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
    ));
    let mut sheets = String::new();
    let mut rels = String::new();
    for (n, name) in (1..).zip(names) {
        types.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        ));
        sheets.push_str(&format!(
            r#"<sheet name="{}" sheetId="{n}" r:id="rId{n}"/>"#,
            escaped(name)
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#
        ));
    }
    types.push_str("</Types>");
    let book = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
            r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
            r#"<sheets>{}</sheets></workbook>"#
        ),
        sheets
    );
    let book_rels = format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}</Relationships>"#
        ),
        rels
    );
    let root_rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>"#,
        r#"</Relationships>"#
    );
    let sheet = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#
    );
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut put = |name: &str, bytes: &[u8]| {
        out.start_file(name, options).expect("start an entry");
        out.write_all(bytes).expect("write an entry");
    };
    put("[Content_Types].xml", types.as_bytes());
    put("_rels/.rels", root_rels.as_bytes());
    put("xl/workbook.xml", book.as_bytes());
    put("xl/_rels/workbook.xml.rels", book_rels.as_bytes());
    for n in 1..=names.len() {
        put(&format!("xl/worksheets/sheet{n}.xml"), sheet.as_bytes());
    }
    out.finish().expect("finish the package").into_inner()
}

/// `text` with the five signs that XML escapes in an attribute escaped.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

use std::collections::BTreeMap;

use anyhow::Result;
use lopdf::{content::Content, Document, Encoding, Object};

pub struct PdfTextIterator<'a> {
    content: Content,
    encodings: BTreeMap<Vec<u8>, Encoding<'a>>,
    current_font: Option<Vec<u8>>,
    pos: usize,
}

pub struct PdfTextDocument {
    document: Document,
}

impl PdfTextDocument {
    pub fn load<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        Ok(Self { document: Document::load(path)? })
    }

    pub fn text_object_iter(&self) -> impl Iterator<Item = TextObject> + '_ {
        self.into_iter().flatten()
    }
}

pub struct PdfTextDocumentIterator<'a> {
    document: &'a Document,
    page_ids: Vec<(u32, u16)>,
    current_page: usize,
}

impl<'a> Iterator for PdfTextDocumentIterator<'a> {
    // Vec<TextObject> is owned — no lifetime tie to Document.
    // Callers can .flatten() this to iterate all TextObjects across pages.
    type Item = PdfTextIterator<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.current_page < self.page_ids.len() {
            let page_id = self.page_ids[self.current_page];
            self.current_page += 1;
            if let Ok(iter) = PdfTextIterator::new(self.document, page_id) {
                return Some(iter);
            }
        }
        None
    }
}

impl<'a> IntoIterator for &'a PdfTextDocument {
    type Item = PdfTextIterator<'a>;
    type IntoIter = PdfTextDocumentIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        let page_ids = self.document.get_pages().into_values().collect();
        PdfTextDocumentIterator { document: &self.document, page_ids, current_page: 0 }
    }
}

impl<'a> PdfTextIterator<'a> {
    pub fn new(document: &'a Document, page_id: (u32, u16)) -> Result<Self> {
        let fonts = document.get_page_fonts(page_id)?;
        let content = Content::decode(&document.get_page_content(page_id)?)?;
        let encodings: BTreeMap<Vec<u8>, Encoding> = fonts
            .into_iter()
            .filter_map(|(name, font)| match font.get_font_encoding(document) {
                Ok(it) => Some((name, it)),
                _ => None,
            })
            .collect();

        Ok(Self {
            content,
            encodings,
            current_font: None,
            pos: 0,
        })
    }
}

pub struct TextObject {
    pub text: String,
    pub x: f32,
    pub y: f32,
}

impl From<TextObject> for String {
    fn from(obj: TextObject) -> Self {
        obj.text
    }
}

/// Transforms a stream of pdf operations into text elements, using
/// `BeginText` and `EndText` as the boundaries.
impl<'a> Iterator for PdfTextIterator<'a> {
    type Item = TextObject;

    fn next(&mut self) -> Option<Self::Item> {
        let mut current_text = String::new();
        let mut current_coords = None;
        let mut in_text_object = false;

        while self.pos < self.content.operations.len() {
            let op = &self.content.operations[self.pos];
            self.pos += 1;

            match op.operator.as_ref() {
                "BT" => {
                    // begin-text
                    in_text_object = true;
                    current_text.clear();
                }
                "Tf" => {
                    // Missing operand means font switch failed; abort rather than
                    // decode subsequent text with a stale/wrong encoding.
                    if let Ok(font) = op.operands.first()?.as_name() {
                        self.current_font = Some(font.to_vec());
                    }
                }
                "Tj" | "TJ" => {
                    if let Some(encoding) = self
                        .current_font
                        .as_deref()
                        .and_then(|font| self.encodings.get(font))
                    {
                        collect_text(&mut current_text, encoding, &op.operands);
                    }
                }
                "T*" => {
                    // new line
                    current_text.push('\n');
                }
                "ET" if in_text_object => {
                    // end-text
                    in_text_object = false;

                    let token = current_text.trim();

                    if !token.is_empty() {
                        if let Some((x, y)) = current_coords.take() {
                            return Some(TextObject {
                                x,
                                y,
                                text: token.to_string(),
                            });
                        }
                    }
                }
                "Tm" => {
                    if let Some([x, y]) = op.operands.get(4..6) {
                        current_coords = Some((
                            x.as_float().unwrap_or_default(),
                            y.as_float().unwrap_or_default(),
                        ));
                    }
                }
                "Td" => {
                    if let Some([x, y]) = op.operands.get(0..2) {
                        current_coords = Some((
                            x.as_float().unwrap_or_default(),
                            y.as_float().unwrap_or_default(),
                        ));
                    }
                }
                _ => (), // skip past everything else
            }
        }

        None
    }
}

fn collect_text(text: &mut String, encoding: &Encoding, operands: &[Object]) {
    for operand in operands.iter() {
        match operand {
            Object::String(bytes, _) => {
                if let Ok(str) = &Document::decode_text(encoding, bytes) {
                    text.push_str(str);
                }
            }
            Object::Array(arr) => {
                collect_text(text, encoding, arr);
            }
            _ => (),
        }
    }
}

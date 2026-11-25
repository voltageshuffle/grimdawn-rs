//! The arguments of a floating text's format tag (event 27's text data).

#[derive(Clone, Debug, PartialEq)]
pub enum TextArg {
    Num(f32),
    /// A text tag, localised by the reader.
    Tag(String),
}

//! Ask-first consumable state: a tonic / writ / bounty right-clicked in the bags.

/// Pending its requirement test and yes / no question.
#[derive(Clone, Debug, PartialEq)]
pub struct UseAsk {
    pub bag: usize,
    pub x: u32,
    pub y: u32,
    pub record: String,
    pub class: String,
    /// The question's text tag once tested.
    pub question: Option<&'static str>,
}

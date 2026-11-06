//! Renderer facts. The renderer records them during its step. The world step applies them in order.
use super::{GeneratedId, SurfaceKey, WorldRequest};
use crate::reference::SourceId;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RendererFact {
    /// The renderer needs the prototype.
    HoldPrototype(SourceId),
    /// The prototype materials are ready.
    BindPrototype(SourceId),
    /// The renderer dropped the prototype.
    ReleasePrototype(SourceId),
    /// The structure materials of the prototype are ready.
    BindRecipe(SourceId),
    /// The renderer started the prepared surface.
    ReserveSurface(WorldRequest, SurfaceKey),
    /// The renderer finished the prepared surface.
    AcceptSurface(WorldRequest, SurfaceKey),
    /// The renderer dropped the prepared surface after its work drained.
    DiscardSurface(WorldRequest, SurfaceKey),
    /// The renderer finished the structure. Its claim moves to the ready queue.
    AcceptStructure(WorldRequest, GeneratedId),
}

/// Facts of one renderer step, in order.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RendererFacts(Vec<RendererFact>);

impl RendererFacts {
    pub fn push(&mut self, fact: RendererFact) {
        self.0.push(fact);
    }
    /// Adds the facts of a later step.
    pub fn extend(&mut self, later: Self) {
        self.0.extend(later.0);
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn contains(&self, fact: &RendererFact) -> bool {
        self.0.contains(fact)
    }
    pub(super) fn drain(&mut self) -> std::vec::Drain<'_, RendererFact> {
        self.0.drain(..)
    }
}

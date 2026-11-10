use std::{fmt, sync::Arc};

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeSeq};

/// An inheritance chain whose clones and children share their unchanged ancestry.
#[derive(Clone, Default)]
pub struct EntityMaterialChain(Option<Arc<Link>>);

struct Link {
    name: Box<str>,
    parent: EntityMaterialChain,
}

impl EntityMaterialChain {
    /// Adds `name` before a shared parent chain without copying its entries.
    #[must_use]
    pub fn prepend(name: Box<str>, parent: Self) -> Self {
        Self(Some(Arc::new(Link { name, parent })))
    }

    /// Reads names from the material to its oldest parent without allocating.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        let mut next = self.0.as_deref();
        std::iter::from_fn(move || {
            let link = next?;
            next = link.parent.0.as_deref();
            Some(link.name.as_ref())
        })
    }
}

impl Drop for EntityMaterialChain {
    fn drop(&mut self) {
        let mut next = self.0.take();
        while let Some(link) = next {
            let Ok(mut link) = Arc::try_unwrap(link) else {
                break;
            };
            next = link.parent.0.take();
        }
    }
}

impl fmt::Debug for EntityMaterialChain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl PartialEq for EntityMaterialChain {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl Eq for EntityMaterialChain {}

impl Serialize for EntityMaterialChain {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.iter().count()))?;
        for name in self.iter() {
            sequence.serialize_element(name)?;
        }
        sequence.end()
    }
}

impl<'de> Deserialize<'de> for EntityMaterialChain {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Vec::<Box<str>>::deserialize(deserializer)?
            .into_iter()
            .rev()
            .fold(Self::default(), |parent, name| Self::prepend(name, parent)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_share_parents_and_serialize_as_name_lists() {
        let parent = EntityMaterialChain::prepend("base".into(), EntityMaterialChain::default());
        let child = EntityMaterialChain::prepend("child".into(), parent.clone());
        assert!(Arc::ptr_eq(
            child.0.as_ref().unwrap().parent.0.as_ref().unwrap(),
            parent.0.as_ref().unwrap(),
        ));
        let json = serde_json::to_value(&child).unwrap();
        assert_eq!(json, serde_json::json!(["child", "base"]));
        assert_eq!(
            serde_json::from_value::<EntityMaterialChain>(json).unwrap(),
            child
        );
    }

    #[test]
    fn deep_chains_walk_and_drop_without_recursion() {
        let mut chain = EntityMaterialChain::default();
        for index in 0..16_384 {
            chain = EntityMaterialChain::prepend(index.to_string().into(), chain);
        }
        assert_eq!(chain.iter().count(), 16_384);
        drop(chain);
    }
}

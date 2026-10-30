//! Compact source initialization inputs for excluded content and nested callers.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldInitialization {
    #[serde(deserialize_with = "compact_vec")]
    pub nodes: Vec<WorldInitNode>,
    #[serde(deserialize_with = "compact_vec")]
    pub components: Vec<WorldInitComponent>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldInitNode {
    pub source: SourceId,
    pub owner: SourceId,
    pub parent: Option<SourceId>,
    #[serde(deserialize_with = "compact_vec")]
    pub children: Vec<SourceId>,
    #[serde(deserialize_with = "compact_vec")]
    pub components: Vec<SourceId>,
    pub active: bool,
    pub pose: Transform,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldInitComponent {
    pub source: SourceId,
    pub owner: SourceId,
    pub class_name: String,
    pub execution_order: i32,
    #[serde(deserialize_with = "compact_vec")]
    pub fields: Vec<WorldInitField>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldInitField {
    pub name: String,
    pub value: WorldValue,
}
impl WorldInitialization {
    pub(super) fn validate(
        &self,
        world: &WorldReference,
        receipts: &HashSet<&SourceId>,
    ) -> Result<(), ReferenceError> {
        let records: HashSet<_> = world
            .records
            .iter()
            .map(|r| &r.source)
            .chain(self.components.iter().map(|r| &r.source))
            .collect();
        let nodes: HashSet<_> = self.nodes.iter().map(|n| &n.source).collect();
        require(
            nodes.len() == self.nodes.len(),
            "initialization duplicate node",
        )?;
        let components: HashSet<_> = self.components.iter().map(|r| &r.source).collect();
        require(
            components.len() == self.components.len(),
            "initialization duplicate component",
        )?;
        for node in &self.nodes {
            require(
                receipts.contains(&node.source) && receipts.contains(&node.owner),
                "initialization node receipts",
            )?;
            require(transform(&node.pose), "initialization transform")?;
            if let Some(parent) = &node.parent {
                require(
                    nodes.contains(parent) || records.contains(parent),
                    "initialization parent input",
                )?;
            }
            for child in &node.children {
                require(
                    nodes.contains(child) || records.contains(child),
                    "initialization child input",
                )?;
            }
            for component in &node.components {
                require(
                    records.contains(component),
                    "initialization component input",
                )?;
            }
        }
        for record in &self.components {
            require(
                receipts.contains(&record.source) && receipts.contains(&record.owner),
                "initialization component receipts",
            )?;
            let mut names = HashSet::new();
            for field in &record.fields {
                let explicit = match record.class_name.as_str() {
                    "tosaveitemscript" => matches!(
                        field.name.as_str(),
                        "partslotscripts"
                            | "randomScales"
                            | "painting"
                            | "spawnBox"
                            | "partconditions"
                    ),
                    "partslotscript" => field.name == "part",
                    "partscript" => field.name == "tosaveitem",
                    "partconditionscript" => field.name == "childs",
                    _ => false,
                };
                fn links(value: &WorldValue, records: &HashSet<&SourceId>) -> bool {
                    match value {
                        WorldValue::Link(source) => records.contains(source),
                        WorldValue::Array(a) => a.iter().all(|v| links(v, records)),
                        WorldValue::Record(f) => f.values().all(|v| links(v, records)),
                        _ => true,
                    }
                }
                if explicit {
                    require(
                        links(&field.value, &records),
                        "initialization FStart dependency",
                    )?;
                }
                require(
                    names.insert(&field.name) && values_valid(&field.value, receipts),
                    "initialization field input",
                )?;
            }
        }
        Ok(())
    }
}

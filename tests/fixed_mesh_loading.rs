use the_long_slop::reference::{
    fixed_reservation::FixedReservation, prototype_plan::PrototypePlan,
};

#[test]
fn fixed_import_counts_unused_meshes_and_shared_node_meshes_once() {
    let primitive = |mesh| {
        serde_json::json!({
            "mesh":mesh,"primitive":0,"submesh":0,"vertices":3,"indices":3,"bytes":48,
            "attributes":["POSITION"],"source_material":null,"has_source_material":false,"material":null
        })
    };
    let node = |index| {
        serde_json::json!({
            "index":index,"mesh":0,"skin":null,"children":[],"matrix":null,
            "translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1],"extras":null
        })
    };
    let plan: PrototypePlan = serde_json::from_value(serde_json::json!({
        "json_bytes":64,"buffer_bytes":96,"mesh_bytes":96,"scene_bytes":128,
        "nodes":[node(0),node(1),node(2)],"primitives":[primitive(0),primitive(1)]
    }))
    .unwrap();
    let bound = FixedReservation::mesh(&plan, 0).unwrap();
    assert_eq!(bound.mesh_bytes, 288);
    assert_eq!(bound.gpu, 304);
}

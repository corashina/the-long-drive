# Parry face feature patch

This directory contains the cached crates.io `parry3d` 0.31.1 package.
Its version, features, and dependency versions are unchanged.
The package keeps its Apache 2.0 license (see `Cargo.toml`).

Upstream crate SHA256: 7b626edc0b497dba4bdf6e1e0797895294ed9f61dac2bd28cb5d34d37ad2ca7e.

The changed upstream file is `src/shape/convex_polyhedron.rs`.

`PolygonalFeatureMap::local_support_feature` returns at most four face vertices.
Upstream takes the first four vertices of the face contour.
On a face with many sides, these four vertices are a thin sliver at the face border.
A box on the top of the garage barrel (a 32-sided hull) then gets one contact point and never rests.
The patch takes four corners that span the face: the longest chord and the vertex farthest from it on each side.

The local copy omits benches, examples, tests, and dev-dependencies.

When upgrading Parry, check if upstream selects large face corners. If it does, remove this patch.

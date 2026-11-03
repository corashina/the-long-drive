# Rapier resting kinematic body patch

This directory contains the cached crates.io `rapier3d` 0.36.0 package.
Its version, features, and dependency versions are unchanged.
The package keeps its Apache 2.0 license (see `Cargo.toml`).

Upstream crate SHA256: c2e2b20538584bb3ba5bfe6a66c2d9a242ee17d15e5398be0fa35279c7d73739.

The changed upstream file is `src/control/character_controller.rs`.

`detect_grounded_status_and_apply_friction` moves the character with each touching kinematic body.
Upstream sets the character move along the contact normal to the body move, in both directions.
A resting kinematic body then also removes moves away from it.
Many garage walls and items are resting kinematic bodies.
A player who touched one and slid along another wall stopped dead.
The patch applies the normal correction only when it pushes the character out.
The tangent carry for moving platforms is unchanged.

The local copy omits examples, tests, and dev-dependencies.

When upgrading Rapier, check if upstream still removes moves away from kinematic bodies. If not, remove this patch.

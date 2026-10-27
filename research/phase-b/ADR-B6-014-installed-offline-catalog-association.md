# ADR B6-014: Observe installed association without promoting historical trust

Status: Linux engine/CLI implemented; source-bound local checks passed. Broader approval/revocation/update and release qualification remain open.

Add an authenticated read-only installed-catalog check under separate model-pack owner authority. Require exact bootstrap namespace, current stored catalog revision and manifest digest. Revalidate the protected accepted capsule, exact original manifest target binding and installed assets/profile/receipt. Check root, timestamp, snapshot and targets expiry before and after asset hashing; refuse time before the accepted clock watermark or backwards movement during the check.

Return a typed point-in-time association report. Preserve all false execution/rights/compatibility/quality/current publisher claims; explicitly deny online revocation checking. A later accepted catalog omission refuses association but does not implicitly change owner selection or existing jobs. Such durable approval/revocation/update policy needs an explicit separate transaction and reader boundary.

No storage mutation, new clock watermark, project schema or portable authority follows. Protected local cooperative state and correct system clock are assumptions. Desktop, current revocation, active update/rollback and platform/media/device/human/release qualification remain required. See ../../docs/PULSAR_INSTALLED_CATALOG_ASSOCIATION.md.

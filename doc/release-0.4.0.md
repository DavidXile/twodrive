# TwoDrive 0.4.0 — DavidXile fork

This release combines the recovered Connections/Files interface with the current Dashboard, including aligned transfer/backup previews. Manual transfer history survives restart; directory display loads independently of aggregate status and rejects stale account/revision/path results.

Linux iCloud and Baidu now use existing TwoDriveFS metadata, content cache and pending-write protections. Complete shallow listings refresh independently of content reads. Cold speculative GIO probes avoid hydration; fully cached content remains available to MIME detection. Migration refuses old unsynchronized VFS/editor recovery content and leaves it intact. Rclone remains the iCloud protocol adapter; no additional filesystem is introduced.

Folder backups update original paths by default and never propagate local deletion. Versioned filenames require explicit selection. Baidu reuses overwrite/revision support. iCloud stages and verifies content, rechecks the original snapshot and journals recoverable replacement; it does not advertise atomic conditional writes. Previous remote objects remain as recovery material and occupy cloud space until deliberately recovered or cleaned up.

Mounted file viewing opens the verified original mount path, so editing participates in filesystem synchronization. Unmounted viewing uses explicit downloads. Busy shutdown identifies holders and offers retry after the user saves/closes those applications normally. It never terminates them. The Linux Flutter shutdown fix keeps both views alive until render workers stop and releases them before input helpers.

The DAV Debian development test package is isolated from the stable package and documents its Nautilus asset limits. Private account state, credentials and file lists are not included in release artifacts.

Validation receipts and CI links will be attached to the release. Synthetic provider/FUSE tests establish mechanism behavior; they do not establish real Apple/Baidu network latency, live-cloud replacement behavior or native NTFS/Explorer acceptance. Production-account migration is recorded separately from test results.

# Atomic generated-file replacement

DeskHalloumi writes generated i3 include files through a same-directory temporary
file and `rename(2)`. The write path preserves the existing mode, flushes and
synchronizes the temporary file, renames it over the destination, and then
synchronizes the parent directory.

## Failure contract

The implementation distinguishes two failure classes:

1. **Before replacement.** Creating, permission-setting, writing, flushing,
   synchronizing, or renaming the temporary file failed. The previous
   destination remains authoritative and the temporary file is removed.
2. **After replacement, before parent-directory synchronization.** The new file
   is already visible at the destination, but persistence of the directory entry
   across a sudden power loss cannot be confirmed. The error explicitly reports
   that the destination was replaced and durability is uncertain.

Transactional i3 installation treats the second class as a changed destination:
it restores the previous include, or removes a newly created include, before
returning the error. A successful candidate write followed by an i3 reload
failure uses the same restoration path and then reloads the restored include.

Non-transactional callers cannot pretend that an after-replacement sync failure
left the old file in place. They receive the explicit uncertainty error and must
decide whether to retry, restore from their own snapshot, or ask the operator to
verify the generated file.

## Testability

The parent-directory synchronization step is injected into the internal helper.
The regression test forces that final step to fail and verifies all three
observable guarantees:

- the error is classified as post-replacement;
- the new destination content is visible;
- no temporary file remains.

# Sharing Files with Other Users

## Overview

Sharing files between users requires coordination so that the
recipient controls access to their storage for a limited duration.

**Workflow:**

1. Recipient creates a world-writable directory.
2. Sender copies files into that directory.
3. Recipient revokes the open permissions.

> Use `/scratch` for sharing — opening permissions on `/home`
> directories is discouraged due to technical issues.

## Step 1 — Recipient Creates a Receiving Directory

Replace `<recipient>` with the actual ASURITE.

```shell
chmod -R o+rx /scratch/<recipient>
install -d -m 777 /scratch/<recipient>/receiving_dir
```

## Step 2 — Sender Copies Files

```shell
chmod o+rwx /path/to/mydirectory_of_files
cp -R /path/to/mydirectory_of_files /scratch/<recipient>/receiving_dir
chmod o-rwx /path/to/mydirectory_of_files
```

The third command reverts permissions on the original files; the copy
on the recipient side is unaffected.

## Step 3 — Recipient Revokes Permissions

```shell
chmod o-rwx /scratch/<recipient>
```

This stops other users from navigating or modifying the shared files.

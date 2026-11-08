use crate::{BLOCK_SIZE, Disk, FileSystem, Node, NodeFlags, Transaction, TreeData, TreePtr};

/// DashFS: what the node keeps beside its data: whether a directory compares names without case (copied while it
/// is still empty, so that names that differ only in case fit), the Amiga bits and the attributes.
fn copy_extras<D: Disk, E: Disk>(
    tx_old: &mut Transaction<D>,
    node_old: &TreeData<Node>,
    tx: &mut Transaction<E>,
    node: &mut TreeData<Node>,
) -> syscall::Result<()> {
    let mut flags = node.data().flags();
    flags.set(
        NodeFlags::CASEFOLD,
        node_old.data().flags().contains(NodeFlags::CASEFOLD),
    );
    node.data_mut().set_flags(flags);
    node.data_mut().set_amiga_bits(node_old.data().amiga_bits());
    let attributes = node_old.data().attributes;
    if !attributes.is_null() {
        let block = tx_old.read_block(attributes)?;
        node.data_mut().attributes = tx.write_attributes(block.data())?;
    }
    Ok(())
}

fn tx_progress<D: Disk, F: FnMut(u64)>(tx: &mut Transaction<D>, progress: &mut F) {
    let size = tx.header.size();
    let free = tx.allocator.free() * BLOCK_SIZE;
    progress(size - free);
}

//TODO: handle hard links
fn clone_at<D: Disk, E: Disk, F: FnMut(u64)>(
    tx_old: &mut Transaction<D>,
    parent_ptr_old: TreePtr<Node>,
    tx: &mut Transaction<E>,
    parent_ptr: TreePtr<Node>,
    buf: &mut [u8],
    progress: &mut F,
) -> syscall::Result<()> {
    let mut entries = Vec::new();
    tx_old.child_nodes(parent_ptr_old, &mut entries)?;
    for entry in entries {
        //TODO: return error instead?
        let Some(name) = entry.name() else {
            continue;
        };
        let node_ptr_old = entry.node_ptr();
        let node_old = tx_old.read_tree(node_ptr_old)?;

        //TODO: this slows down the clone, but Redox has issues without this (Linux is fine)
        if tx.write_cache.len() > 64 {
            tx.sync(false)?;
        }

        let node_ptr = {
            let mode = node_old.data().mode();
            let (ctime, ctime_nsec) = node_old.data().ctime();
            let (mtime, mtime_nsec) = node_old.data().mtime();
            let mut node = tx.create_node(parent_ptr, &name, mode, ctime, ctime_nsec)?;
            node.data_mut().set_uid(node_old.data().uid());
            node.data_mut().set_gid(node_old.data().gid());
            node.data_mut().set_mtime(mtime, mtime_nsec);
            copy_extras(tx_old, &node_old, tx, &mut node)?;

            if !node_old.data().is_dir() {
                let mut offset = 0;
                loop {
                    let count = tx_old.read_node_inner(&node_old, offset, buf)?;
                    if count == 0 {
                        break;
                    }
                    tx.write_node_inner(&mut node, &mut offset, &buf[..count])?;
                }
            }

            let node_ptr = node.ptr();
            tx.sync_tree(node)?;
            node_ptr
        };

        tx_progress(tx, progress);

        if node_old.data().is_dir() {
            clone_at(tx_old, node_ptr_old, tx, node_ptr, buf, progress)?;
        }
    }

    Ok(())
}

pub fn clone<D: Disk, E: Disk, F: FnMut(u64)>(
    fs_old: &mut FileSystem<D>,
    fs: &mut FileSystem<E>,
    mut progress: F,
) -> syscall::Result<()> {
    fs_old.tx(|tx_old| {
        let mut tx = Transaction::new(fs);

        // Clone at root node, which is still empty
        let root_old = tx_old.read_tree(TreePtr::root())?;
        let mut root = tx.read_tree(TreePtr::root())?;
        copy_extras(tx_old, &root_old, &mut tx, &mut root)?;
        tx.sync_tree(root)?;

        // Clone at root node
        let mut buf = vec![0; 4 * 1024 * 1024];
        clone_at(
            tx_old,
            TreePtr::root(),
            &mut tx,
            TreePtr::root(),
            &mut buf,
            &mut progress,
        )?;

        // Commit and squash alloc log
        tx.commit(true)
    })
}

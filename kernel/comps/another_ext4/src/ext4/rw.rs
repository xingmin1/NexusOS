use super::Ext4;
use crate::constants::*;
use crate::ext4_defs::*;
use crate::prelude::*;

impl Ext4 {
    /// Read a block from block device
    pub(super) fn read_block(&self, block_id: PBlockId) -> Block {
        #[cfg(feature = "block_cache")]
        {
            self.block_cache.read_block(block_id)
        }
        #[cfg(not(feature = "block_cache"))]
        {
            self.block_device.read_block(block_id)
        }
    }

    /// Write a block to block device
    pub(super) fn write_block(&self, block: &Block) {
        #[cfg(feature = "block_cache")]
        {
            self.block_cache.write_block(block)
        }
        #[cfg(not(feature = "block_cache"))]
        {
            self.block_device.write_block(block)
        }
    }

    /// Read super block from block device
    #[allow(unused)]
    pub(super) fn read_super_block(&self) -> SuperBlock {
        let block = self.read_block(0);
        block.read_offset_as(BASE_OFFSET)
    }

    /// Write super block to block device
    pub(super) fn write_super_block(&self, sb: &SuperBlock) {
        let mut block = Block::new(0, [0; BLOCK_SIZE]);
        block.write_offset_as(BASE_OFFSET, sb);
        self.write_block(&block)
    }

    /// Read an inode from block device, return an `InodeRef` that
    /// combines the inode and its id.
    pub(super) fn read_inode(&self, inode_id: InodeId) -> InodeRef {
        let (block_id, offset) = self.inode_disk_pos(inode_id);
        let block = self.read_block(block_id);
        
        InodeRef::new(inode_id, block.read_offset_as(offset))
    }

    /// Read the root inode from block device
    #[allow(unused)]
    pub(super) fn read_root_inode(&self) -> InodeRef {
        self.read_inode(EXT4_ROOT_INO)
    }

    /// Write an inode to block device with checksum
    pub(super) fn write_inode_with_csum(&self, inode_ref: &mut InodeRef) {
        let super_block = self.read_super_block();
        inode_ref.set_checksum(&super_block.uuid());
        self.write_inode_without_csum(inode_ref);
    }

    /// Write an inode to block device without checksum
    pub(super) fn write_inode_without_csum(&self, inode_ref: &InodeRef) {
        let (block_id, offset) = self.inode_disk_pos(inode_ref.id);
        let mut block = self.read_block(block_id);
        block.write_offset_as(offset, &inode_ref.inode);
        self.write_block(&block)
    }

    /// Read a block group descriptor from block device, return an `BlockGroupRef`
    /// that combines the block group descriptor and its id.
    pub(super) fn read_block_group(&self, block_group_id: BlockGroupId) -> BlockGroupRef {
        let (block_id, offset) = self.block_group_disk_pos(block_group_id);
        let block = self.read_block(block_id as PBlockId);
        BlockGroupRef::new(
            block_group_id,
            block.read_offset_as::<BlockGroupDesc>(offset),
        )
    }

    /// Write a block group descriptor to block device with checksum
    pub(super) fn write_block_group_with_csum(&self, bg_ref: &mut BlockGroupRef) {
        let super_block = self.read_super_block();
        bg_ref.set_checksum(&super_block.uuid());
        self.write_block_group_without_csum(bg_ref);
    }

    /// Write a block group descriptor to block device without checksum
    #[allow(unused)]
    pub(super) fn write_block_group_without_csum(&self, bg_ref: &BlockGroupRef) {
        let (block_id, offset) = self.block_group_disk_pos(bg_ref.id);
        let mut block = self.read_block(block_id as PBlockId);
        block.write_offset_as(offset, &bg_ref.desc);
        self.write_block(&block);
    }

    /// Get disk position of an inode. Return block id and offset within the block.
    ///
    /// Each block group contains `sb.inodes_per_group` inodes.
    /// Because inode 0 is defined not to exist, this formula can
    /// be used to find the block group that an inode lives in:
    /// `bg = (inode_id - 1) / sb.inodes_per_group`.
    ///
    /// The particular inode can be found within the block group's
    /// inode table at `index = (inode_id - 1) % sb.inodes_per_group`.
    /// To get the byte address within the inode table, use
    /// `offset = index * sb.inode_size`.
    fn inode_disk_pos(&self, inode_id: InodeId) -> (PBlockId, usize) {
        let super_block = self.read_super_block();
        let inodes_per_group = super_block.inodes_per_group();

        let bg_id = ((inode_id - 1) / inodes_per_group) as BlockGroupId;
        let inode_size = super_block.inode_size();
        let bg = self.read_block_group(bg_id);
        let id_in_bg = ((inode_id - 1) % inodes_per_group) as usize;

        let block_id =
            bg.desc.inode_table_first_block() + (id_in_bg * inode_size / BLOCK_SIZE) as PBlockId;
        let offset = (id_in_bg * inode_size) % BLOCK_SIZE;
        (block_id, offset)
    }

    /// Get disk position of a block group. Return block id and offset within the block.
    fn block_group_disk_pos(&self, block_group_id: BlockGroupId) -> (PBlockId, usize) {
        let super_block = self.read_super_block();
        let desc_per_block = BLOCK_SIZE as u32 / super_block.desc_size() as u32;

        let block_id = super_block.first_data_block() + block_group_id / desc_per_block + 1;
        let offset = (block_group_id % desc_per_block) * super_block.desc_size() as u32;
        (block_id as PBlockId, offset as usize)
    }
}

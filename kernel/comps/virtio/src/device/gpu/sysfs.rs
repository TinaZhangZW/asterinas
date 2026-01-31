// SPDX-License-Identifier: MPL-2.0

use alloc::{string::ToString, sync::Arc};

use aster_systree::{
	AttrLessBranchNodeFields, NormalNodeFields, Result as SysResult, SysAttrSetBuilder, SysObj,
	SysPerms, SysStr, SymlinkNodeFields, inherit_sys_branch_node, inherit_sys_leaf_node,
	inherit_sys_symlink_node,
};
use aster_util::printer::VmPrinter;
use ostd::mm::VmWriter;
use spin::Once;

#[derive(Debug)]
struct SimpleBranchNode {
	fields: AttrLessBranchNodeFields<dyn SysObj, Self>,
}

impl SimpleBranchNode {
	fn new(name: SysStr) -> Arc<Self> {
		Arc::new_cyclic(|weak_self| {
			let fields = AttrLessBranchNodeFields::new(name, weak_self.clone());
			Self { fields }
		})
	}

	fn add_child(&self, new_child: Arc<dyn SysObj>) -> SysResult<()> {
		self.fields.add_child(new_child)
	}

	fn child(&self, name: &str) -> Option<Arc<dyn SysObj>> {
		self.fields.child(name)
	}
}

inherit_sys_branch_node!(SimpleBranchNode, fields, {
	fn perms(&self) -> SysPerms {
		SysPerms::DEFAULT_RO_PERMS
	}
});

#[derive(Debug)]
struct SimpleSymlinkNode {
	fields: SymlinkNodeFields<Self>,
}

impl SimpleSymlinkNode {
	fn new(name: SysStr, target: &str) -> Arc<Self> {
		Arc::new_cyclic(|weak_self| {
			let fields = SymlinkNodeFields::new(name, target.to_string(), weak_self.clone());
			Self { fields }
		})
	}
}

inherit_sys_symlink_node!(SimpleSymlinkNode, fields);

#[derive(Debug)]
struct GraphicsFbNode {
	fields: NormalNodeFields<Self>,
}

impl GraphicsFbNode {
	fn new(name: SysStr) -> Arc<Self> {
		let mut builder = SysAttrSetBuilder::new();
		builder.add("name".into(), SysPerms::DEFAULT_RO_ATTR_PERMS);
		let attrs = builder.build().expect("Failed to build sysfs attributes");

		Arc::new_cyclic(|weak_self| {
			let fields = NormalNodeFields::new(name, attrs, weak_self.clone());
			Self { fields }
		})
	}
}

inherit_sys_leaf_node!(GraphicsFbNode, fields, {
	fn read_attr_at(&self, name: &str, offset: usize, writer: &mut VmWriter) -> SysResult<usize> {
		if name != "name" {
			return Err(aster_systree::Error::NotFound);
		}
		let mut printer = VmPrinter::new_skip(writer, offset);
		write!(printer, "virtio-gpu\n")?;
		Ok(printer.bytes_written())
	}

	fn perms(&self) -> SysPerms {
		SysPerms::DEFAULT_RO_PERMS
	}
});

struct SysfsRoots {
	virtio_gpu: Arc<SimpleBranchNode>,
	drm: Arc<SimpleBranchNode>,
	graphics: Arc<SimpleBranchNode>,
}

static SYSFS_ROOTS: Once<Arc<SysfsRoots>> = Once::new();

fn add_child_ignore_exists(parent: &SimpleBranchNode, child: Arc<dyn SysObj>) {
	let _ = parent.add_child(child);
}

fn init_sysfs_roots() -> Arc<SysfsRoots> {
	let root = aster_systree::primary_tree().root().clone();

	let bus = SimpleBranchNode::new(SysStr::from("bus"));
	let class = SimpleBranchNode::new(SysStr::from("class"));
	let _ = root.add_child(bus.clone());
	let _ = root.add_child(class.clone());

	let virtio = SimpleBranchNode::new(SysStr::from("virtio"));
	add_child_ignore_exists(&bus, virtio.clone());
	let drivers = SimpleBranchNode::new(SysStr::from("drivers"));
	add_child_ignore_exists(&virtio, drivers.clone());
	let virtio_gpu = SimpleBranchNode::new(SysStr::from("virtio_gpu"));
	add_child_ignore_exists(&drivers, virtio_gpu.clone());

	let drm = SimpleBranchNode::new(SysStr::from("drm"));
	add_child_ignore_exists(&class, drm.clone());
	let graphics = SimpleBranchNode::new(SysStr::from("graphics"));
	add_child_ignore_exists(&class, graphics.clone());

	Arc::new(SysfsRoots {
		virtio_gpu,
		drm,
		graphics,
	})
}

fn ensure_symlink(parent: &SimpleBranchNode, name: &str, target: &str) {
	if parent.child(name).is_some() {
		return;
	}
	let node = SimpleSymlinkNode::new(SysStr::from(name.to_string()), target);
	add_child_ignore_exists(parent, node);
}

pub fn register_virtio_gpu_sysfs(virtio_index: u32, drm_index: u32) -> SysResult<()> {
	let roots = SYSFS_ROOTS.call_once(init_sysfs_roots);

	let virtio_name = alloc::format!("virtio{}", virtio_index);
	add_child_ignore_exists(&roots.virtio_gpu, SimpleBranchNode::new(SysStr::from(virtio_name)));

	let card_name = alloc::format!("card{}", drm_index);
	let card = SimpleBranchNode::new(SysStr::from(card_name));
	add_child_ignore_exists(&roots.drm, card.clone());

	let device = SimpleBranchNode::new(SysStr::from("device"));
	add_child_ignore_exists(&card, device.clone());
	ensure_symlink(&device, "driver", "/sys/bus/virtio/drivers/virtio_gpu");

	if roots.graphics.child("fb0").is_none() {
		let fb = GraphicsFbNode::new(SysStr::from("fb0"));
		add_child_ignore_exists(&roots.graphics, fb);
	}

	Ok(())
}
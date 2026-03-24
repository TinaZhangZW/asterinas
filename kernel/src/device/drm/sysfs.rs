use alloc::{borrow::Cow, collections::BTreeMap, format, string::String, sync::Arc};

use aster_systree::{
    BranchNodeFields, Error as SysTreeError, Result as SysTreeResult, SymlinkNodeFields,
    SysAttrSetBuilder, SysObj, SysPerms, SysStr, inherit_sys_branch_node, inherit_sys_symlink_node,
};
use aster_util::printer::VmPrinter;
use inherit_methods_macro::inherit_methods;
use ostd::{
    mm::{VmReader, VmWriter},
    sync::RwLock,
};
use spin::Once;

use super::minor::{DrmMinor, DrmMinorType};
use crate::{fs::sysfs, prelude::*};

static SYS_DEV_ROOT: Once<Arc<StaticDirNode>> = Once::new();
static SYS_DEV_CHAR_ROOT: Once<Arc<StaticDirNode>> = Once::new();
static SYS_CLASS_ROOT: Once<Arc<StaticDirNode>> = Once::new();
static SYS_CLASS_DRM_ROOT: Once<Arc<StaticDirNode>> = Once::new();

const VIRTIO_VENDOR_ID: u16 = 0x1af4;
const VIRTIO_GPU_DEVICE_ID: u16 = 0x1050;

#[derive(Debug)]
struct StaticDirNode {
    fields: BranchNodeFields<dyn SysObj, Self>,
    attrs: RwLock<BTreeMap<String, String>>,
}

#[inherit_methods(from = "self.fields")]
impl StaticDirNode {
    fn new(name: impl Into<SysStr>, attrs: BTreeMap<String, String>) -> Arc<Self> {
        let mut builder = SysAttrSetBuilder::new();
        for key in attrs.keys() {
            builder.add(Cow::Owned(key.clone()), SysPerms::DEFAULT_RO_ATTR_PERMS);
        }
        let attr_set = builder.build().expect("failed to build sysfs attribute set");

        Arc::new_cyclic(|weak_self| Self {
            fields: BranchNodeFields::new(name.into(), attr_set, weak_self.clone()),
            attrs: RwLock::new(attrs),
        })
    }

    fn add_child(&self, new_child: Arc<dyn SysObj>) -> SysTreeResult<()>;

    fn child(&self, name: &str) -> Option<Arc<dyn SysObj>>;
}

inherit_sys_branch_node!(StaticDirNode, fields, {
    fn read_attr_at(&self, name: &str, offset: usize, writer: &mut VmWriter) -> SysTreeResult<usize> {
        let attrs = self.attrs.read();
        let value = attrs.get(name).ok_or(SysTreeError::NotFound)?;

        let mut printer = VmPrinter::new_skip(writer, offset);
        write!(printer, "{}", value)?;

        Ok(printer.bytes_written())
    }

    fn write_attr(&self, _name: &str, _reader: &mut VmReader) -> SysTreeResult<usize> {
        Err(SysTreeError::AttributeError)
    }

    fn perms(&self) -> SysPerms {
        SysPerms::DEFAULT_RW_PERMS
    }
});

#[derive(Debug)]
struct StaticSymlinkNode {
    fields: SymlinkNodeFields<Self>,
}

impl StaticSymlinkNode {
    fn new(name: impl Into<SysStr>, target: impl Into<String>) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| Self {
            fields: SymlinkNodeFields::new(name.into(), target.into(), weak_self.clone()),
        })
    }
}

inherit_sys_symlink_node!(StaticSymlinkNode, fields);

fn add_child_ignore_exists(parent: &Arc<StaticDirNode>, child: Arc<dyn SysObj>) -> Result<()> {
    match parent.add_child(child) {
        Ok(()) => Ok(()),
        Err(SysTreeError::AlreadyExists) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn ensure_sys_dev_char_root() -> Result<&'static Arc<StaticDirNode>> {
    if SYS_DEV_ROOT.get().is_none() {
        let dev_root = StaticDirNode::new("dev", BTreeMap::new());
        sysfs::systree_singleton().root().add_child(dev_root.clone())?;
        let _ = SYS_DEV_ROOT.call_once(|| dev_root);
    }

    let dev_root = SYS_DEV_ROOT.get().unwrap();

    if SYS_DEV_CHAR_ROOT.get().is_none() {
        let char_root = StaticDirNode::new("char", BTreeMap::new());
        add_child_ignore_exists(dev_root, char_root.clone())?;
        let _ = SYS_DEV_CHAR_ROOT.call_once(|| char_root);
    }

    Ok(SYS_DEV_CHAR_ROOT.get().unwrap())
}

fn ensure_sys_class_drm_root() -> Result<&'static Arc<StaticDirNode>> {
    if SYS_CLASS_ROOT.get().is_none() {
        let class_root = StaticDirNode::new("class", BTreeMap::new());
        sysfs::systree_singleton().root().add_child(class_root.clone())?;
        let _ = SYS_CLASS_ROOT.call_once(|| class_root);
    }

    let class_root = SYS_CLASS_ROOT.get().unwrap();

    if SYS_CLASS_DRM_ROOT.get().is_none() {
        let drm_root = StaticDirNode::new("drm", BTreeMap::new());
        add_child_ignore_exists(class_root, drm_root.clone())?;
        let _ = SYS_CLASS_DRM_ROOT.call_once(|| drm_root);
    }

    Ok(SYS_CLASS_DRM_ROOT.get().unwrap())
}

fn default_pci_attrs(index: u32) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();

    attrs.insert("uevent".to_string(), format!("PCI_SLOT_NAME=0000:00:{:02x}.0\n", index & 0xff));
    attrs.insert("revision".to_string(), "0x00\n".to_string());
    attrs.insert("vendor".to_string(), format!("0x{:04x}\n", VIRTIO_VENDOR_ID));
    attrs.insert("device".to_string(), format!("0x{:04x}\n", VIRTIO_GPU_DEVICE_ID));
    attrs.insert(
        "subsystem_vendor".to_string(),
        format!("0x{:04x}\n", VIRTIO_VENDOR_ID),
    );
    attrs.insert(
        "subsystem_device".to_string(),
        format!("0x{:04x}\n", VIRTIO_GPU_DEVICE_ID),
    );

    attrs
}

pub(super) fn register_minor(minor: &Arc<DrmMinor>) -> Result<()> {
    let char_root = ensure_sys_dev_char_root()?;
    let class_drm_root = ensure_sys_class_drm_root()?;

    let (major, minor_id) = minor.major_minor();
    let dev_char_name = format!("{}:{}", major, minor_id);

    if char_root.child(&dev_char_name).is_some() {
        return Ok(());
    }

    let mut dev_char_attrs = BTreeMap::new();
    dev_char_attrs.insert("dev".to_string(), format!("{}:{}\n", major, minor_id));
    dev_char_attrs.insert(
        "uevent".to_string(),
        format!(
            "MAJOR={}\nMINOR={}\nDEVNAME=dri/{}\nDEVTYPE=drm_minor\n",
            major,
            minor_id,
            minor.node_basename()
        ),
    );

    let dev_char_node = StaticDirNode::new(dev_char_name.clone(), dev_char_attrs);
    add_child_ignore_exists(char_root, dev_char_node.clone())?;

    let node_name = minor.node_basename();
    let mut class_attrs = BTreeMap::new();
    class_attrs.insert("dev".to_string(), format!("{}:{}\n", major, minor_id));
    class_attrs.insert(
        "uevent".to_string(),
        format!(
            "MAJOR={}\nMINOR={}\nDEVNAME=dri/{}\nDEVTYPE=drm_minor\n",
            major, minor_id, node_name
        ),
    );
    let class_node = StaticDirNode::new(node_name.clone(), class_attrs);
    add_child_ignore_exists(class_drm_root, class_node.clone())?;
    add_child_ignore_exists(
        &class_node,
        StaticSymlinkNode::new("subsystem", "/sys/class/drm"),
    )?;
    add_child_ignore_exists(
        &class_node,
        StaticSymlinkNode::new("device", format!("/sys/dev/char/{}/device", dev_char_name)),
    )?;

    let device_node = StaticDirNode::new("device", default_pci_attrs(minor.index()));
    add_child_ignore_exists(&dev_char_node, device_node.clone())?;

    let subsystem_link = StaticSymlinkNode::new("subsystem", "/sys/bus/pci");
    add_child_ignore_exists(&device_node, subsystem_link)?;

    let drm_dir = StaticDirNode::new("drm", BTreeMap::new());
    add_child_ignore_exists(&device_node, drm_dir.clone())?;
    add_child_ignore_exists(&drm_dir, StaticDirNode::new(node_name, BTreeMap::new()))?;

    // Keep both common node names visible under one DRM device path so tools
    // that scan /sys/.../device/drm can discover primary+render topology.
    if !matches!(minor.type_(), DrmMinorType::Accel) {
        let idx = minor.index();
        add_child_ignore_exists(
            &drm_dir,
            StaticDirNode::new(format!("card{}", idx), BTreeMap::new()),
        )?;
        add_child_ignore_exists(
            &drm_dir,
            StaticDirNode::new(format!("renderD{}", 128 + idx), BTreeMap::new()),
        )?;
    }

    Ok(())
}

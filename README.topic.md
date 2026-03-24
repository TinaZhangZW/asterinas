# From OpenGL Baseline to Vulkan Enablement: Validating Virtio-GPU Compatibility for Mesa 3D in Asterinas

## Background

Modern Linux graphics userspace depends on more than a single GPU driver entry point. To run standard Mesa userspace on a kernel, the guest-side virtio-gpu implementation must expose a sufficiently Linux-compatible contract across:

- DRM core ioctls
- virtio-gpu-specific ioctls
- capability and capset reporting
- blob resource handling
- memory mapping behavior
- context initialization
- synchronization and fence behavior

This project focuses on Asterinas virtio-gpu compatibility for Mesa 3D userspace.

## Starting Point

Before this contest work, the Asterinas virtio-gpu frontend had already reached a useful OpenGL baseline. In practice, it was already possible to run:

- `kmscube`
- `xfce`
- `glxgears` inside the desktop session

That means the basic OpenGL path was already working to a meaningful extent. The contest goal is therefore not to claim only "basic OpenGL works", but to extend that baseline to Vulkan and explain the result as a dual-API compatibility story.

## Goal

The goal of this project is:

Enable Vulkan on top of the existing OpenGL virtio-gpu baseline in Asterinas, and validate the result as compatibility for both OpenGL and Vulkan APIs over one virtio-gpu frontend.

More concretely, the project aims to:

1. keep the existing OpenGL path working
2. enable the Vulkan userspace path
3. validate the shared virtio-gpu guest contract used by Mesa VirGL and Mesa Venus
4. document supported behavior, partial behavior, and known limitations

## Why This Topic Matters

- It is directly relevant to Asterinas kernel capability growth.
- It combines kernel work, DRM/KMS UAPI, Mesa userspace, and graphics debugging.
- It is demonstrable within the contest time window.
- It produces reusable engineering knowledge, not just one demo result.

## Technical Framing

The same Asterinas virtio-gpu frontend should support two major guest graphics API paths:

- OpenGL through Mesa VirGL
- Vulkan through Mesa Venus

These paths overlap heavily in the guest-kernel contract they require. The most important areas are:

- `GETPARAM`
- `GET_CAPS`
- `RESOURCE_CREATE_BLOB`
- `MAP`
- `CONTEXT_INIT`
- `EXECBUFFER`
- syncobj and fence behavior

This makes the project a compatibility and validation problem, not only a feature implementation task.

## Current Implementation Scope

The current Asterinas virtio-gpu work already includes support for important 3D guest-facing paths such as:

- `DRM_IOCTL_VIRTGPU_GETPARAM`
- `DRM_IOCTL_VIRTGPU_GET_CAPS`
- `DRM_IOCTL_VIRTGPU_RESOURCE_CREATE`
- `DRM_IOCTL_VIRTGPU_RESOURCE_INFO`
- `DRM_IOCTL_VIRTGPU_MAP`
- `DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST`
- `DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST`
- `DRM_IOCTL_VIRTGPU_WAIT`
- `DRM_IOCTL_VIRTGPU_EXECBUFFER`
- `DRM_IOCTL_VIRTGPU_RESOURCE_CREATE_BLOB`
- `DRM_IOCTL_VIRTGPU_CONTEXT_INIT`
- DRM syncobj and timeline syncobj support

These are the core pieces needed to explain the result as a Mesa compatibility effort.

## Main Work Items

The contest work is focused on the following:

1. Vulkan enablement
   - make `vulkaninfo` work
   - make `vkcube` work

2. OpenGL non-regression
   - keep `kmscube` working
   - optionally keep `xfce` + `glxgears` working as stronger OpenGL evidence

3. Compatibility validation
   - verify the virtio-gpu parameters and capsets expected by Mesa
   - validate blob resource and host-visible mapping behavior
   - validate context initialization and ring behavior
   - validate synchronization semantics

4. Documentation
   - summarize supported and unsupported behavior
   - record debugging methodology
   - extract reusable lessons for other Asterinas developers

## Validation Plan

### Low-Level Validation

Low-level validation is based on `user/playground/virtiogpu_test`, especially:

- `GETPARAM`
- `GET_CAPS`
- `RESOURCE_CREATE`
- `RESOURCE_INFO`
- `MAP`
- `RESOURCE_CREATE_BLOB`
- `CONTEXT_INIT`
- `EXECBUFFER`
- `SYNC_DRM`
- `SYNC_DRM_TIMELINE`

### OpenGL Baseline Validation

- `kmscube`
- optional `xfce` + `glxgears`

### Vulkan Validation

- `vulkaninfo`
- `vkcube`

## Success Criteria

This project is considered successful if:

1. the low-level virtio-gpu test path is stable enough to support the claim
2. the existing OpenGL baseline remains valid
3. `vulkaninfo` can enumerate the Vulkan virtio-gpu path
4. `vkcube` can run
5. the remaining limitations are clearly documented

## Key Risks

The main current risks are:

1. `EXECBUFFER` fence and completion semantics
2. Venus capability and capset handshake correctness
3. blob and host-visible memory behavior
4. context-init and ring behavior
5. syncobj and external fence behavior under real Vulkan startup

Broader DRM/KMS gaps exist too, but they are not the center of this project:

- incomplete vblank behavior
- partial atomic/KMS behavior
- cursor limitations
- compositor-grade desktop semantics

## Non-Goals

The following are intentionally not primary goals of this submission:

- full Linux DRM parity
- making full desktop compositor support the main milestone
- implementing every optional virtio-gpu path
- performance optimization before correctness
- unrelated graphics features outside the VirGL/Venus compatibility story

## How Coding Agents Were Used

Coding agents were used to:

- inspect Mesa and Linux UAPI expectations
- compare Asterinas behavior against upstream contracts
- generate compatibility matrices and bug lists
- narrow the work to the highest-value blockers
- turn debugging and implementation work into reusable documentation

All agent-produced results still require manual verification.

## Expected Output

The expected output of this project is:

- a working Vulkan milestone on top of the existing OpenGL baseline
- a validated compatibility story for both VirGL and Venus guest paths
- a reproducible validation process
- documentation that other Asterinas developers can reuse

## Short Project Pitch

This project starts from a real OpenGL virtio-gpu baseline in Asterinas and pushes the graphics stack one level further by enabling Vulkan. The contribution is not only that graphics applications can run, but that the guest-side virtio-gpu contract is analyzed, validated, and explained in a way that covers both Mesa VirGL and Mesa Venus. That makes the result useful both as a contest demo and as a reusable engineering asset for future Asterinas graphics work.

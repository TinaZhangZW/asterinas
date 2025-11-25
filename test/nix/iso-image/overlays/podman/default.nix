self: super: {
  runc = super.runc.overrideAttrs (oldAttrs: {
    patches = (oldAttrs.patches or [ ])
      ++ [ ./patches/0001-Patch-runc-for-Asterinas.patch ];
  });
  podman = (super.podman.overrideAttrs (oldAttrs: {
    patches = (oldAttrs.patches or [ ])
      ++ [ ./patches/0001-Patch-podman-for-Asterinas.patch ];
  })).override { runc = self.runc; };
}

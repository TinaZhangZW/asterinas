self: super:

{
  asterinas-docs = super.stdenv.mkDerivation {
    name = "asterinas-docs";
    src = ../docs;
    installPhase = ''
      mkdir -p $out
      cp -r $src/* $out/
    '';
  };
}
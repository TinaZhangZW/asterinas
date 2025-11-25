self: super: {
  hello = super.stdenv.mkDerivation {
    name = "hello-0.1.0";
    buildCommand = ''
      mkdir -p $out/bin
      cat > $out/bin/hello << 'EOF'
      #!/bin/sh
      echo "Hello Asterinas!"
      EOF
      chmod +x $out/bin/hello
    '';
  };
}

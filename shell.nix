with import <nixpkgs> { }; 

runCommand "dummy" {
    buildInputs = [ cargo rustc gcc automake autoconf perl pkg-config ];
} ""

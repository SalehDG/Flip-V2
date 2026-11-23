Flip V2 - Portable Mode (Source Patch)

Portable mode is enabled by placing an empty file named:
    portable.flag
in the same folder as the built Flip V2 executable.


Portable folder layout expected at runtime:
    Flip V2 Portable/
        FlipV2.exe
        portable.flag
        data/
            flip-v2.sqlite3
            attachments/
        config/
            oauth-client.json
            diagnostics/

The application uses the data/ and config/ folders beside the executable when
portable.flag is present. Without portable.flag, it continues using the standard
Windows application data/config locations.


Gmail OAuth refresh tokens are stored in Windows Credential Manager, not in
portable files. When moving the portable folder to another computer, Gmail
accounts will need to authenticate again. This is intentionally safer than
exporting tokens in a portable package.


Build verification note:
This source patch has not been compiled in this environment because Rust/Cargo
is unavailable. Compile and test it on the Windows development machine before
sharing the application.

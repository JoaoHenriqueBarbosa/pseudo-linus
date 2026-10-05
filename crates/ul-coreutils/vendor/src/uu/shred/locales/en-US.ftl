shred-about = Overwrite the specified FILE(s) repeatedly, in order to make it harder
    for even very expensive hardware probing to recover the data.
shred-usage = shred [OPTION]... FILE...
shred-after-help = If FILE is -, shred standard output.

    Delete FILE(s) if --remove (-u) is specified.  The default is not to remove
    the files because it is common to operate on device files like /dev/hda,
    and those files usually should not be removed.
    The optional HOW parameter indicates how to remove a directory entry:
    'unlink' => use a standard unlink call.
    'wipe' => also first obfuscate bytes in the name.
    'wipesync' => also sync each obfuscated byte to the device.
    The default mode is 'wipesync', but note it can be expensive.
shred-help-force = change permissions to allow writing if necessary
shred-help-iterations = overwrite N times instead of the default (3)
shred-help-random-source = get random bytes from FILE
shred-help-size = shred this many bytes (suffixes like K, M, G accepted)
shred-help-u = deallocate and remove file after overwriting
shred-help-remove = like -u but give control on HOW to delete;  See below
shred-help-verbose = show progress
shred-help-exact = do not round file sizes up to the next full block;
    this is the default for non-regular files
shred-help-zero = add a final overwrite with zeros to hide shredding

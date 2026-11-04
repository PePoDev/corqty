;; A precompiled Component Model executor using the contract in crates/plugin-sdk/wit.
;; The canonical ABI records at 0, 8 and 16 point into static UTF-8 data.
(component
  (core module $m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32) (param $size i32) (result i32)
      (local $ptr i32)
      global.get $heap local.tee $ptr
      local.get $size i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap
      local.get $ptr)
    (data (i32.const 0) "\40\00\00\00\54\00\00\00")
    (data (i32.const 8) "\a0\00\00\00\02\00\00\00")
    (data (i32.const 16) "\00\00\00\00\b0\00\00\00\1c\00\00\00")
    (data (i32.const 64) "{\22key\22:\22hello\22,\22version\22:\221.0.0\22,\22hostContract\22:\221\22,\22capabilities\22:[{\22type\22:\22log\22}]}")
    (data (i32.const 160) "[]")
    (data (i32.const 176) "{\22result\22:\22hello from wasm\22}")
    (func (export "metadata") (result i32) i32.const 0)
    (func (export "validate") (param i32 i32) (result i32) i32.const 8)
    (func (export "execute") (param i32 i32) (result i32) i32.const 16)
  )
  (core instance $i (instantiate $m))
  (alias core export $i "memory" (core memory $memory))
  (alias core export $i "realloc" (core func $realloc))
  (func (export "metadata") (result string)
    (canon lift (core func $i "metadata") (memory $memory) (realloc $realloc)))
  (func (export "validate") (param "config-json" string) (result string)
    (canon lift (core func $i "validate") (memory $memory) (realloc $realloc)))
  (func (export "execute") (param "request-json" string) (result (result string (error string)))
    (canon lift (core func $i "execute") (memory $memory) (realloc $realloc)))
)

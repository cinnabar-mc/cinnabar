(component
  (type $viewport (record (field "width" f32) (field "height" f32) (field "scale" f32)))
  (type $font (enum "native" "sans"))
  (type $element (enum "crosshair" "status-effects" "sidebar" "hunger"))
  (import "$HUD" (instance $hud
    (export "viewport" (type $v (eq $viewport)))
    (export "font" (type $f (eq $font)))
    (export "vanilla-element" (type $e (eq $element)))
    (export "set-surface" (func (param "json" string) (result (result (error string)))))
    (export "update-surface" (func (param "json" string) (result (result (error string)))))
    (export "read-viewport" (func (result (option $v))))
    (export "measure-text" (func (param "text" string) (param "font" $f) (result (result f32 (error string)))))
    (export "set-hidden-elements" (func (param "elements" (list $e)) (result (result (error string)))))))
  (alias export $hud "set-surface" (func $set))
  (alias export $hud "update-surface" (func $update))
  (alias export $hud "read-viewport" (func $viewport))
  (alias export $hud "measure-text" (func $measure))
  (alias export $hud "set-hidden-elements" (func $hide))
  (core module $memory-module
    (memory (export "memory") 4)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 131072))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-set (canon lower (func $set) (memory $memory) (realloc $realloc)))
  (core func $lower-update (canon lower (func $update) (memory $memory) (realloc $realloc)))
  (core func $lower-viewport (canon lower (func $viewport) (memory $memory) (realloc $realloc)))
  (core func $lower-measure (canon lower (func $measure) (memory $memory) (realloc $realloc)))
  (core func $lower-hide (canon lower (func $hide) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "set" (func $set (param i32 i32 i32)))
    (import "host" "update" (func $update (param i32 i32 i32)))
    (import "host" "viewport" (func $viewport (param i32)))
    (import "host" "measure" (func $measure (param i32 i32 i32 i32)))
    (import "host" "hide" (func $hide (param i32 i32 i32)))
    (import "host" "memory" (memory 4))
    (data (i32.const 0) "$SURFACE")
    (data (i32.const 65536) "$PATCH")
    (data (i32.const 98304) "$TEXT")
    (data (i32.const 98400) "\00\03")
    ;; Results land at 98560; viewport reads at 98600; measured widths are stored at 98700.
    (func $surface
      i32.const 0 i32.const $SURFACE_LENGTH i32.const 98560 call $set
      i32.const 98560 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func $patch
      i32.const 65536 i32.const $PATCH_LENGTH i32.const 98560 call $update
      i32.const 98560 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func $clear
      i32.const 0 i32.const 0 i32.const 98560 call $set)
    (func $hide-two
      i32.const 98400 i32.const 2 i32.const 98560 call $hide
      i32.const 98560 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func $show-all
      i32.const 98400 i32.const 0 i32.const 98560 call $hide)
    (func $require-viewport
      i32.const 98600 call $viewport
      i32.const 98600 i32.load8_u i32.eqz if unreachable end
      i32.const 98604 f32.load f32.const $WIDTH f32.ne if unreachable end
      i32.const 98612 f32.load f32.const $SCALE f32.ne if unreachable end)
    (func $require-no-viewport
      i32.const 98600 call $viewport
      i32.const 98600 i32.load8_u if unreachable end)
    (func $measure-sans
      i32.const 98304 i32.const $TEXT_LENGTH i32.const 1 i32.const 98560 call $measure
      i32.const 98560 i32.load8_u i32.const $ERROR i32.ne if unreachable end
      i32.const 98700 i32.const 98564 f32.load f32.store)
    (func (export "init") $INIT)
    (func (export "frame") $FRAME))
  (core instance $host
    (export "set" (func $lower-set))
    (export "update" (func $lower-update))
    (export "viewport" (func $lower-viewport))
    (export "measure" (func $lower-measure))
    (export "hide" (func $lower-hide))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))

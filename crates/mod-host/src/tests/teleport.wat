(component
  (import "cinnabar:extension/teleport-handling@0.1.0" (instance $types
    (type $r (record (field "yaw" f32) (field "pitch" f32)))
    (export "rotation" (type $rotation (eq $r)))
    (type $m (enum "normal" "reset" "teleport" "rotation" "other"))
    (export "teleport-mode" (type $mode (eq $m)))
    (type $c (record (field "mode" $mode) (field "server" $rotation)
      (field "presented" $rotation) (field "rotation" $rotation)))
    (export "teleport-context" (type (eq $c)))))
  (alias export $types "teleport-context" (type $context))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096)
    (func (export "init"))
    (func (export "frame"))
    ;; mode, server yaw/pitch, presented yaw/pitch, rotation yaw/pitch
    (func (export "handle") (param i32 f32 f32 f32 f32 f32 f32) (result i32)
      $BODY
      i32.const 512 local.get 0 i32.store8
      i32.const 516 $SERVER_YAW f32.store
      i32.const 520 local.get 2 f32.store
      i32.const 524 local.get 3 f32.store
      i32.const 528 local.get 4 f32.store
      i32.const 532 $YAW f32.store
      i32.const 536 $PITCH f32.store
      i32.const 512))
  (core instance $i (instantiate $m))
  (alias core export $i "memory" (core memory $memory))
  (alias core export $i "realloc" (core func $realloc))
  (func (export "init") (canon lift (core func $i "init")))
  (func (export "frame") (canon lift (core func $i "frame")))
  (func (export "handle-teleport") (param "ctx" $context) (result $context)
    (canon lift (core func $i "handle") (memory $memory) (realloc $realloc))))

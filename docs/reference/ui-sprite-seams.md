# Shared pixel edges for sliced images

A nine-sliced image is one surface. Its adjacent pieces must meet on the same
physical pixel edge. Snapping each piece's origin and rounding its width and
height upward independently can overlap a row or column. A half-transparent
black background then blends twice at the joint, producing a darker line even
when its source image is uniformly black.

Emitted pieces retain their whole image's bounds relative to the piece's origin.
This survives cached layout translations. Offset and size animations move both
the piece and its parent bounds; clip animations crop the piece while retaining
the full image's grid. Painting preserves the ordinary image's snapped outer
bounds and assigns shared internal edges once on that grid. Tooltip backgrounds
use the same partition rule. Unsplit images keep their existing placement rule.

The synthetic engine regression checks that every covered pixel belongs to
exactly one piece at multiple GUI scales and display DPIs. It failed before the
change with coverage two at a slice joint. Numeric tests preserve outer bounds
and shared edges, including negative origins; animation tests check moving,
resizing, clipping and translated cached draws. No game textures are embedded.
An in-game comparison between unpatched Cinnabar nightly and the patched client
confirms that the dark horizontal seam disappears from translucent HUD panels.
Broader native comparisons of transformed and animated sliced artwork remain
open acceptance work.

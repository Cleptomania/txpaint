Family: Single-line
    Shape: Rectangle
        TL: 218
        TR: 191
        BL: 192
        BR: 217
        H: 196
        V: 179
    
    Shape: Connected
        T_UP: 193
        T_DOWN: 194
        T_LEFT: 180
        T_RIGHT: 195
        CROSS: 197

Family: Double-line
    Shape: Rectangle
        TL: 201
        TR: 187
        BL: 200
        BR: 188
        H: 205
        V: 186

    Shape: Connected
        T_UP: 202
        T_DOWN: 203
        T_LEFT: 185
        T_RIGHT: 204
        CROSS: 206

  CrossFamily: Single-line <-> Double-line

      # Corners. Naming: <corner>_<vert>_<horiz>
      # <corner> = TL (top-left), TR (top-right), BL (bottom-left), BR (bottom-right)
      # <vert>/<horiz> = S (single) or D (double), for each arm of the corner.
      # Example: TL_S_D means top-left corner where the vertical arm is single and
      # the horizontal arm is double.
      TL_S_D: 213
      TL_D_S: 214
      TR_S_D: 184
      TR_D_S: 183
      BL_S_D: 212
      BL_D_S: 211
      BR_S_D: 190
      BR_D_S: 189

      # T-junctions. Naming: T_<leg>_<line>_<perp>
      # <leg> = UP/DOWN/LEFT/RIGHT (direction the stem of the T points)
      # <line> = family of the straight-through arm (the pair opposite the leg)
      # <perp> = family of the leg (stem) arm itself
      T_UP_S_D: 208
      T_UP_D_S: 207
      T_DOWN_S_D: 210
      T_DOWN_D_S: 209
      T_LEFT_S_D: 181
      T_LEFT_D_S: 182
      T_RIGHT_S_D: 198
      T_RIGHT_D_S: 199

      # Crosses. Naming: CROSS_<horiz>_<vert>
      CROSS_S_D: 215
      CROSS_D_S: 216
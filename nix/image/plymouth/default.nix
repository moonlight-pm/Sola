# Sola Plymouth theme — the five-petal mark *is* the spinner.
# Each petal cycles neon cyan shades so a bright wave walks around the flower.
#
# Geometry: petals.json (absolute paths from flower.svg).

{
  lib,
  stdenvNoCC,
  librsvg,
  python3,
}:

let
  # One frame per peak position (5 petals) — discrete clockwise walk.
  nframes = 5;
  # Plymouth refresh is ~50 Hz; skip this many refreshes between frame advances
  # so each petal step lasts ~300 ms (15 / 50 ≈ 0.3 s). Full rev ≈ 1.5 s.
  refreshesPerFrame = 15;
in
stdenvNoCC.mkDerivation {
  pname = "plymouth-theme-sola";
  version = "0.3.0";

  src = ./.;

  nativeBuildInputs = [
    librsvg
    python3
  ];

  buildPhase = ''
    runHook preBuild
    mkdir -p frames
    python3 gen-frames.py petals.json frames ${toString nframes} 360
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    themeDir=$out/share/plymouth/themes/sola
    mkdir -p "$themeDir"
    cp frames/frame-*.png "$themeDir/"

    cp frames/frame-00.png "$out/logo.png"
    mkdir -p $out/share/plymouth
    cp frames/frame-00.png $out/share/plymouth/sola-logo.png

    cat > "$themeDir/sola.plymouth" <<EOF
[Plymouth Theme]
Name=Sola
Description=Five-petal flower with neon cyan petal cycle
ModuleName=script

[script]
ImageDir=$themeDir
ScriptFile=$themeDir/sola.script
EOF

    # Explicit frame list — Plymouth script string ops are weak.
    {
      echo '# Sola boot mark — flower is the spinner; petals cycle neon cyan.'
      echo 'Window.SetBackgroundTopColor(0.04, 0.045, 0.055);'
      echo 'Window.SetBackgroundBottomColor(0.02, 0.025, 0.03);'
      echo
      echo 'flower.num_frames = ${toString nframes};'
      echo 'flower.frames = [];'
      for i in $(seq 0 $((${toString nframes} - 1))); do
        printf 'flower.frames[%d] = Image("frame-%02d.png");\n' "$i" "$i"
      done
      echo
      echo 'flower.index = 0;'
      echo 'flower.tick = 0;'
      echo 'flower.refreshes_per_frame = ${toString refreshesPerFrame};'
      echo 'flower.sprite = Sprite(flower.frames[0]);'
      echo 'flower.sprite.SetZ(100);'
      echo
      echo 'fun place_flower() {'
      echo '  flower.sprite.SetX(Window.GetX() + Window.GetWidth()  / 2 - flower.frames[flower.index].GetWidth()  / 2);'
      echo '  flower.sprite.SetY(Window.GetY() + Window.GetHeight() / 2 - flower.frames[flower.index].GetHeight() / 2);'
      echo '}'
      echo 'place_flower();'
      echo
      echo 'fun refresh_callback() {'
      # Advance ~once per 300 ms (not every Plymouth refresh).
      echo '  flower.tick = flower.tick + 1;'
      echo '  if (flower.tick < flower.refreshes_per_frame)'
      echo '    return;'
      echo '  flower.tick = 0;'
      echo '  flower.index = (flower.index + 1) % flower.num_frames;'
      echo '  flower.sprite.SetImage(flower.frames[flower.index]);'
      echo '  place_flower();'
      echo '}'
      echo 'Plymouth.SetRefreshFunction(refresh_callback);'
    } > "$themeDir/sola.script"

    runHook postInstall
  '';

  meta = with lib; {
    description = "Sola Plymouth theme — cyan-cycling five-petal flower";
    platforms = platforms.linux;
  };
}

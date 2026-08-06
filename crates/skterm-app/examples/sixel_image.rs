fn main() {
    // Sixel Red and Blue checker square
    let red_band = "#1;2;100;0;0!40~";
    let blue_band = "#2;2;0;50;100!40~";
    print!("\x1bPq\"1;1;40;36");
    for _ in 0..3 {
        print!("{}-", red_band);
        print!("{}-", blue_band);
    }
    println!("\x1b\\");
    println!("Sixel Graphic Output Displayed Successfully!");
}

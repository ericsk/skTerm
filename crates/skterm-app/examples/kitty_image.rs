use base64::Engine;

fn main() {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..8 {
            rgba.push((x * 32) as u8);
            rgba.push((y * 32) as u8);
            rgba.push(200);
            rgba.push(255);
        }
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&rgba);
    print!("\x1b_Ga=T,f=32,s=8,v=8,m=0;{}\x1b\\", b64);
    println!("Kitty Graphics Output Displayed Successfully!");
}

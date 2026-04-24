/// Replaces all instances of a character in a string with a specified character.
///
/// # Arguments
///
/// * `change_from` - The old character.
///
/// * `change_to` - The new character.
///
/// # Returns
///
/// A copy of the input string with the specified character replaced.
pub fn change_char(input_string: String, change_from: char, change_to: char) -> String {
    let mut updated_string = String::new();
    for c in input_string.chars() {
        if c == change_from {
            updated_string.push(change_to);
        } else {
            updated_string.push(c);
        }
    }
    updated_string
}

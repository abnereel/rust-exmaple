use prost::Message;
use prost_live::pb::*;

fn main() {
    let phones = vec![PhoneNumber::new("111-222-333", PhoneType::Home)];
    let person = Person::new("Abner Lee", 1, "abnereel@gmail.com", phones);
    let v1 = person.encode_to_vec();
    let v2 = person.encode_length_delimited_to_vec();
    let person1 = Person::decode(v1.as_ref()).unwrap();
    assert_eq!(person, person1);

    let json = serde_json::to_string_pretty(&person1).unwrap();
    println!("{}", json);
}
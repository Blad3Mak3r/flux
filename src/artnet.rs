use crate::dmx::DMX_CHANNELS;

const ARTNET_ID: &[u8; 8] = b"Art-Net\0";
const OP_DMX: u16 = 0x5000;
const ARTNET_PROTOCOL_VERSION: u16 = 14;
const ART_DMX_HEADER_LEN: usize = 18;

#[derive(Debug, Eq, PartialEq)]
pub struct ArtDmx<'a> {
    pub port_address: u16,
    pub data: &'a [u8],
}

#[derive(Debug, Eq, PartialEq)]
pub enum ParseError {
    Truncated,
    InvalidId,
    UnsupportedOpcode(u16),
    UnsupportedProtocolVersion(u16),
    InvalidPortAddress(u16),
    InvalidLength(usize),
}

pub fn parse_art_dmx(packet: &[u8]) -> Result<ArtDmx<'_>, ParseError> {
    if packet.len() < ART_DMX_HEADER_LEN {
        return Err(ParseError::Truncated);
    }
    if &packet[..ARTNET_ID.len()] != ARTNET_ID {
        return Err(ParseError::InvalidId);
    }

    let opcode = u16::from_le_bytes([packet[8], packet[9]]);
    if opcode != OP_DMX {
        return Err(ParseError::UnsupportedOpcode(opcode));
    }

    let protocol_version = u16::from_be_bytes([packet[10], packet[11]]);
    if protocol_version < ARTNET_PROTOCOL_VERSION {
        return Err(ParseError::UnsupportedProtocolVersion(protocol_version));
    }

    let port_address = u16::from_le_bytes([packet[14], packet[15]]);
    if port_address > 0x7fff {
        return Err(ParseError::InvalidPortAddress(port_address));
    }

    let length = usize::from(u16::from_be_bytes([packet[16], packet[17]]));
    if !(2..=DMX_CHANNELS).contains(&length) || length % 2 != 0 {
        return Err(ParseError::InvalidLength(length));
    }
    if packet.len() < ART_DMX_HEADER_LEN + length {
        return Err(ParseError::Truncated);
    }

    Ok(ArtDmx {
        port_address,
        data: &packet[ART_DMX_HEADER_LEN..ART_DMX_HEADER_LEN + length],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(universe: u16, data: &[u8]) -> Vec<u8> {
        assert!(data.len().is_multiple_of(2));
        let mut packet = Vec::with_capacity(ART_DMX_HEADER_LEN + data.len());
        packet.extend_from_slice(ARTNET_ID);
        packet.extend_from_slice(&OP_DMX.to_le_bytes());
        packet.extend_from_slice(&ARTNET_PROTOCOL_VERSION.to_be_bytes());
        packet.extend_from_slice(&[0, 0]); // sequence and physical
        packet.extend_from_slice(&universe.to_le_bytes());
        packet.extend_from_slice(&(data.len() as u16).to_be_bytes());
        packet.extend_from_slice(data);
        packet
    }

    #[test]
    fn parses_a_valid_art_dmx_packet() {
        let input = packet(23, &[1, 2, 3, 4]);
        assert_eq!(
            parse_art_dmx(&input),
            Ok(ArtDmx {
                port_address: 23,
                data: &[1, 2, 3, 4]
            })
        );
    }

    #[test]
    fn rejects_an_incorrect_header() {
        let mut input = packet(0, &[0, 0]);
        input[0] = b'X';
        assert_eq!(parse_art_dmx(&input), Err(ParseError::InvalidId));
    }

    #[test]
    fn rejects_an_incorrect_opcode() {
        let mut input = packet(0, &[0, 0]);
        input[8..10].copy_from_slice(&0x2000_u16.to_le_bytes());
        assert_eq!(
            parse_art_dmx(&input),
            Err(ParseError::UnsupportedOpcode(0x2000))
        );
    }

    #[test]
    fn rejects_truncated_packets() {
        assert_eq!(parse_art_dmx(b"Art-Net\0"), Err(ParseError::Truncated));

        let mut input = packet(0, &[1, 2]);
        input.pop();
        assert_eq!(parse_art_dmx(&input), Err(ParseError::Truncated));
    }

    #[test]
    fn rejects_invalid_lengths() {
        let mut input = packet(0, &[1, 2]);
        input[16..18].copy_from_slice(&1_u16.to_be_bytes());
        assert_eq!(parse_art_dmx(&input), Err(ParseError::InvalidLength(1)));

        input[16..18].copy_from_slice(&513_u16.to_be_bytes());
        assert_eq!(parse_art_dmx(&input), Err(ParseError::InvalidLength(513)));
    }

    #[test]
    fn decodes_the_port_address_as_little_endian() {
        let input = packet(0x1234, &[1, 2]);
        assert_eq!(parse_art_dmx(&input).unwrap().port_address, 0x1234);
    }

    #[test]
    fn parses_a_full_payload() {
        let payload = vec![42; DMX_CHANNELS];
        assert_eq!(parse_art_dmx(&packet(0, &payload)).unwrap().data, payload);
    }

    #[test]
    fn parses_a_partial_payload() {
        assert_eq!(parse_art_dmx(&packet(0, &[8, 9])).unwrap().data, &[8, 9]);
    }
}

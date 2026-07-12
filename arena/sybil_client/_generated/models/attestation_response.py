from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.attestation_response_pcr_values import AttestationResponsePcrValues





T = TypeVar("T", bound="AttestationResponse")



@_attrs_define
class AttestationResponse:
    """ Development-only JSON projection of an enclave attestation.

    These fields correspond to values carried by an AWS Nitro attestation
    document, but this DTO is not itself the canonical CBOR/COSE document. A
    response with `is_stub = true` has no cryptographic trust value.

        Attributes:
            enclave_pubkey (str): Lowercase hex encoding of Nitro's optional DER-encoded `public_key`
                field. Empty in the development stub.
            is_stub (bool): Always true for the currently implemented development-only response.
            pcr_values (AttestationResponsePcrValues): PCR index to lowercase hex-encoded measurement bytes. A real Nitro
                document uses SHA-384 PCR values; the development stub returns no PCRs.
            report_data (str): Lowercase hex encoding of protocol data carried in Nitro's optional
                `user_data` field. Empty in the development stub.
            signature (str): Base64url encoding of the COSE_Sign1 signature bytes. Empty in the
                development stub; this field alone is insufficient to verify Nitro PKI.
     """

    enclave_pubkey: str
    is_stub: bool
    pcr_values: AttestationResponsePcrValues
    report_data: str
    signature: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.attestation_response_pcr_values import AttestationResponsePcrValues
        enclave_pubkey = self.enclave_pubkey

        is_stub = self.is_stub

        pcr_values = self.pcr_values.to_dict()

        report_data = self.report_data

        signature = self.signature


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "enclave_pubkey": enclave_pubkey,
            "is_stub": is_stub,
            "pcr_values": pcr_values,
            "report_data": report_data,
            "signature": signature,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.attestation_response_pcr_values import AttestationResponsePcrValues
        d = dict(src_dict)
        enclave_pubkey = d.pop("enclave_pubkey")

        is_stub = d.pop("is_stub")

        pcr_values = AttestationResponsePcrValues.from_dict(d.pop("pcr_values"))




        report_data = d.pop("report_data")

        signature = d.pop("signature")

        attestation_response = cls(
            enclave_pubkey=enclave_pubkey,
            is_stub=is_stub,
            pcr_values=pcr_values,
            report_data=report_data,
            signature=signature,
        )


        attestation_response.additional_properties = d
        return attestation_response

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties

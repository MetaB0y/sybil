from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.signed_attestation_dto import SignedAttestationDto





T = TypeVar("T", bound="ResolveMarketRequest")



@_attrs_define
class ResolveMarketRequest:
    """ 
        Attributes:
            payout_nanos (int): Payout per YES share. Integer nanodollars; 1_000_000_000 = $1.
                Payouts are per-share probabilities in [0, 1e9]. Example: 1000000000.
            attestation (None | SignedAttestationDto | Unset):
     """

    payout_nanos: int
    attestation: None | SignedAttestationDto | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.signed_attestation_dto import SignedAttestationDto
        payout_nanos = self.payout_nanos

        attestation: dict[str, Any] | None | Unset
        if isinstance(self.attestation, Unset):
            attestation = UNSET
        elif isinstance(self.attestation, SignedAttestationDto):
            attestation = self.attestation.to_dict()
        else:
            attestation = self.attestation


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "payout_nanos": payout_nanos,
        })
        if attestation is not UNSET:
            field_dict["attestation"] = attestation

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.signed_attestation_dto import SignedAttestationDto
        d = dict(src_dict)
        payout_nanos = d.pop("payout_nanos")

        def _parse_attestation(data: object) -> None | SignedAttestationDto | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                attestation_type_1 = SignedAttestationDto.from_dict(data)



                return attestation_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SignedAttestationDto | Unset, data)

        attestation = _parse_attestation(d.pop("attestation", UNSET))


        resolve_market_request = cls(
            payout_nanos=payout_nanos,
            attestation=attestation,
        )


        resolve_market_request.additional_properties = d
        return resolve_market_request

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

from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.qmdb_state_exclusion_proof_response import QmdbStateExclusionProofResponse
  from ..models.qmdb_state_inclusion_proof_response import QmdbStateInclusionProofResponse





T = TypeVar("T", bound="StateProofResponse")



@_attrs_define
class StateProofResponse:
    """ 
        Attributes:
            block_height (int):
            leaf_key_hex (str):
            proof_format (str):
            proof_kind (str):
            state_root (str):
            state_slot (str):
            verified (bool):
            exclusion_proof (None | QmdbStateExclusionProofResponse | Unset):
            inclusion_proof (None | QmdbStateInclusionProofResponse | Unset):
            leaf_key_ascii (None | str | Unset):
            leaf_value_hex (None | str | Unset):
     """

    block_height: int
    leaf_key_hex: str
    proof_format: str
    proof_kind: str
    state_root: str
    state_slot: str
    verified: bool
    exclusion_proof: None | QmdbStateExclusionProofResponse | Unset = UNSET
    inclusion_proof: None | QmdbStateInclusionProofResponse | Unset = UNSET
    leaf_key_ascii: None | str | Unset = UNSET
    leaf_value_hex: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.qmdb_state_exclusion_proof_response import QmdbStateExclusionProofResponse
        from ..models.qmdb_state_inclusion_proof_response import QmdbStateInclusionProofResponse
        block_height = self.block_height

        leaf_key_hex = self.leaf_key_hex

        proof_format = self.proof_format

        proof_kind = self.proof_kind

        state_root = self.state_root

        state_slot = self.state_slot

        verified = self.verified

        exclusion_proof: dict[str, Any] | None | Unset
        if isinstance(self.exclusion_proof, Unset):
            exclusion_proof = UNSET
        elif isinstance(self.exclusion_proof, QmdbStateExclusionProofResponse):
            exclusion_proof = self.exclusion_proof.to_dict()
        else:
            exclusion_proof = self.exclusion_proof

        inclusion_proof: dict[str, Any] | None | Unset
        if isinstance(self.inclusion_proof, Unset):
            inclusion_proof = UNSET
        elif isinstance(self.inclusion_proof, QmdbStateInclusionProofResponse):
            inclusion_proof = self.inclusion_proof.to_dict()
        else:
            inclusion_proof = self.inclusion_proof

        leaf_key_ascii: None | str | Unset
        if isinstance(self.leaf_key_ascii, Unset):
            leaf_key_ascii = UNSET
        else:
            leaf_key_ascii = self.leaf_key_ascii

        leaf_value_hex: None | str | Unset
        if isinstance(self.leaf_value_hex, Unset):
            leaf_value_hex = UNSET
        else:
            leaf_value_hex = self.leaf_value_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "block_height": block_height,
            "leaf_key_hex": leaf_key_hex,
            "proof_format": proof_format,
            "proof_kind": proof_kind,
            "state_root": state_root,
            "state_slot": state_slot,
            "verified": verified,
        })
        if exclusion_proof is not UNSET:
            field_dict["exclusion_proof"] = exclusion_proof
        if inclusion_proof is not UNSET:
            field_dict["inclusion_proof"] = inclusion_proof
        if leaf_key_ascii is not UNSET:
            field_dict["leaf_key_ascii"] = leaf_key_ascii
        if leaf_value_hex is not UNSET:
            field_dict["leaf_value_hex"] = leaf_value_hex

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.qmdb_state_exclusion_proof_response import QmdbStateExclusionProofResponse
        from ..models.qmdb_state_inclusion_proof_response import QmdbStateInclusionProofResponse
        d = dict(src_dict)
        block_height = d.pop("block_height")

        leaf_key_hex = d.pop("leaf_key_hex")

        proof_format = d.pop("proof_format")

        proof_kind = d.pop("proof_kind")

        state_root = d.pop("state_root")

        state_slot = d.pop("state_slot")

        verified = d.pop("verified")

        def _parse_exclusion_proof(data: object) -> None | QmdbStateExclusionProofResponse | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                exclusion_proof_type_1 = QmdbStateExclusionProofResponse.from_dict(data)



                return exclusion_proof_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | QmdbStateExclusionProofResponse | Unset, data)

        exclusion_proof = _parse_exclusion_proof(d.pop("exclusion_proof", UNSET))


        def _parse_inclusion_proof(data: object) -> None | QmdbStateInclusionProofResponse | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                inclusion_proof_type_1 = QmdbStateInclusionProofResponse.from_dict(data)



                return inclusion_proof_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | QmdbStateInclusionProofResponse | Unset, data)

        inclusion_proof = _parse_inclusion_proof(d.pop("inclusion_proof", UNSET))


        def _parse_leaf_key_ascii(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        leaf_key_ascii = _parse_leaf_key_ascii(d.pop("leaf_key_ascii", UNSET))


        def _parse_leaf_value_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        leaf_value_hex = _parse_leaf_value_hex(d.pop("leaf_value_hex", UNSET))


        state_proof_response = cls(
            block_height=block_height,
            leaf_key_hex=leaf_key_hex,
            proof_format=proof_format,
            proof_kind=proof_kind,
            state_root=state_root,
            state_slot=state_slot,
            verified=verified,
            exclusion_proof=exclusion_proof,
            inclusion_proof=inclusion_proof,
            leaf_key_ascii=leaf_key_ascii,
            leaf_value_hex=leaf_value_hex,
        )


        state_proof_response.additional_properties = d
        return state_proof_response

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

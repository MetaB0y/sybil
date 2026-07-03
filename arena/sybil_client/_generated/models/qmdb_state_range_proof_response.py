from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="QmdbStateRangeProofResponse")



@_attrs_define
class QmdbStateRangeProofResponse:
    """ 
        Attributes:
            digests_hex (list[str]):
            leaves (int):
            ops_root_hex (str):
            unfolded_prefix_peaks_hex (list[str]):
            partial_chunk_digest_hex (None | str | Unset):
            pre_prefix_acc_hex (None | str | Unset):
     """

    digests_hex: list[str]
    leaves: int
    ops_root_hex: str
    unfolded_prefix_peaks_hex: list[str]
    partial_chunk_digest_hex: None | str | Unset = UNSET
    pre_prefix_acc_hex: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        digests_hex = self.digests_hex



        leaves = self.leaves

        ops_root_hex = self.ops_root_hex

        unfolded_prefix_peaks_hex = self.unfolded_prefix_peaks_hex



        partial_chunk_digest_hex: None | str | Unset
        if isinstance(self.partial_chunk_digest_hex, Unset):
            partial_chunk_digest_hex = UNSET
        else:
            partial_chunk_digest_hex = self.partial_chunk_digest_hex

        pre_prefix_acc_hex: None | str | Unset
        if isinstance(self.pre_prefix_acc_hex, Unset):
            pre_prefix_acc_hex = UNSET
        else:
            pre_prefix_acc_hex = self.pre_prefix_acc_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "digests_hex": digests_hex,
            "leaves": leaves,
            "ops_root_hex": ops_root_hex,
            "unfolded_prefix_peaks_hex": unfolded_prefix_peaks_hex,
        })
        if partial_chunk_digest_hex is not UNSET:
            field_dict["partial_chunk_digest_hex"] = partial_chunk_digest_hex
        if pre_prefix_acc_hex is not UNSET:
            field_dict["pre_prefix_acc_hex"] = pre_prefix_acc_hex

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        digests_hex = cast(list[str], d.pop("digests_hex"))


        leaves = d.pop("leaves")

        ops_root_hex = d.pop("ops_root_hex")

        unfolded_prefix_peaks_hex = cast(list[str], d.pop("unfolded_prefix_peaks_hex"))


        def _parse_partial_chunk_digest_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        partial_chunk_digest_hex = _parse_partial_chunk_digest_hex(d.pop("partial_chunk_digest_hex", UNSET))


        def _parse_pre_prefix_acc_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        pre_prefix_acc_hex = _parse_pre_prefix_acc_hex(d.pop("pre_prefix_acc_hex", UNSET))


        qmdb_state_range_proof_response = cls(
            digests_hex=digests_hex,
            leaves=leaves,
            ops_root_hex=ops_root_hex,
            unfolded_prefix_peaks_hex=unfolded_prefix_peaks_hex,
            partial_chunk_digest_hex=partial_chunk_digest_hex,
            pre_prefix_acc_hex=pre_prefix_acc_hex,
        )


        qmdb_state_range_proof_response.additional_properties = d
        return qmdb_state_range_proof_response

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

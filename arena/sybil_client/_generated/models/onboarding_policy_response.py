from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="OnboardingPolicyResponse")



@_attrs_define
class OnboardingPolicyResponse:
    """ Public self-service onboarding stock and server-assigned demo grant.

        Attributes:
            account_capacity (int): Lifetime account-id ceiling for anonymous onboarding. Account ids are
                never reclaimed or reused.
            accounts_allocated (int): Durable non-system account ids already allocated on this chain.
            accounts_remaining (int): Remaining anonymous allocations under the current deployment policy.
            enabled (bool): Whether another anonymous account can currently be allocated.
            grant_nanos (str): Fixed play-money balance assigned by the server to each new public
                account. Integer nanodollars; 1_000_000_000 = $1.
     """

    account_capacity: int
    accounts_allocated: int
    accounts_remaining: int
    enabled: bool
    grant_nanos: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_capacity = self.account_capacity

        accounts_allocated = self.accounts_allocated

        accounts_remaining = self.accounts_remaining

        enabled = self.enabled

        grant_nanos = self.grant_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_capacity": account_capacity,
            "accounts_allocated": accounts_allocated,
            "accounts_remaining": accounts_remaining,
            "enabled": enabled,
            "grant_nanos": grant_nanos,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_capacity = d.pop("account_capacity")

        accounts_allocated = d.pop("accounts_allocated")

        accounts_remaining = d.pop("accounts_remaining")

        enabled = d.pop("enabled")

        grant_nanos = d.pop("grant_nanos")

        onboarding_policy_response = cls(
            account_capacity=account_capacity,
            accounts_allocated=accounts_allocated,
            accounts_remaining=accounts_remaining,
            enabled=enabled,
            grant_nanos=grant_nanos,
        )


        onboarding_policy_response.additional_properties = d
        return onboarding_policy_response

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

from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.account_response import AccountResponse
from ...models.onboard_account_request import OnboardAccountRequest
from typing import cast



def _get_kwargs(
    *,
    body: OnboardAccountRequest,

) -> dict[str, Any]:
    headers: dict[str, Any] = {}


    

    

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/onboarding/accounts",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> AccountResponse | Any | None:
    if response.status_code == 200:
        response_200 = AccountResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = cast(Any, None)
        return response_400

    if response.status_code == 409:
        response_409 = cast(Any, None)
        return response_409

    if response.status_code == 429:
        response_429 = cast(Any, None)
        return response_429

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[AccountResponse | Any]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: OnboardAccountRequest,

) -> Response[AccountResponse | Any]:
    """ POST /v1/onboarding/accounts — allocate one capped public account.

     The server supplies the fixed grant. The API lock covers the durable-stock
    read and atomic account/key command, so concurrent callers cannot overshoot
    the lifetime ceiling.

    Args:
        body (OnboardAccountRequest): Public self-service account onboarding.

            The server, not the caller, chooses the play-money grant. Keeping funding
            out of this DTO prevents anonymous callers from turning account allocation
            into an arbitrary minting interface.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountResponse | Any]
     """


    kwargs = _get_kwargs(
        body=body,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    *,
    client: AuthenticatedClient | Client,
    body: OnboardAccountRequest,

) -> AccountResponse | Any | None:
    """ POST /v1/onboarding/accounts — allocate one capped public account.

     The server supplies the fixed grant. The API lock covers the durable-stock
    read and atomic account/key command, so concurrent callers cannot overshoot
    the lifetime ceiling.

    Args:
        body (OnboardAccountRequest): Public self-service account onboarding.

            The server, not the caller, chooses the play-money grant. Keeping funding
            out of this DTO prevents anonymous callers from turning account allocation
            into an arbitrary minting interface.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountResponse | Any
     """


    return sync_detailed(
        client=client,
body=body,

    ).parsed

async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: OnboardAccountRequest,

) -> Response[AccountResponse | Any]:
    """ POST /v1/onboarding/accounts — allocate one capped public account.

     The server supplies the fixed grant. The API lock covers the durable-stock
    read and atomic account/key command, so concurrent callers cannot overshoot
    the lifetime ceiling.

    Args:
        body (OnboardAccountRequest): Public self-service account onboarding.

            The server, not the caller, chooses the play-money grant. Keeping funding
            out of this DTO prevents anonymous callers from turning account allocation
            into an arbitrary minting interface.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountResponse | Any]
     """


    kwargs = _get_kwargs(
        body=body,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: OnboardAccountRequest,

) -> AccountResponse | Any | None:
    """ POST /v1/onboarding/accounts — allocate one capped public account.

     The server supplies the fixed grant. The API lock covers the durable-stock
    read and atomic account/key command, so concurrent callers cannot overshoot
    the lifetime ceiling.

    Args:
        body (OnboardAccountRequest): Public self-service account onboarding.

            The server, not the caller, chooses the play-money grant. Keeping funding
            out of this DTO prevents anonymous callers from turning account allocation
            into an arbitrary minting interface.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountResponse | Any
     """


    return (await asyncio_detailed(
        client=client,
body=body,

    )).parsed
